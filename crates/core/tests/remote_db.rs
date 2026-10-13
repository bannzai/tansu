//! Turso Cloud の DB (`TANSU_DB_URL=libsql://...`) を開き、ローカルのファイルと同じ操作が通ることを確かめる。
//! 環境変数 `TANSU_DB_URL` / `TANSU_DB_AUTH_TOKEN` が要るため `#[ignore]` にし、CI の `remote` job
//! (workflow_dispatch の時だけ動く) が `-- --ignored` で実行する。接続先は CI 用の DB で、run をまたいで残り、
//! 別の run が同時に書くこともあるため、テーブルを消さず、自分の run が書いた行だけを確かめて消す。

use serde_json::json;
use tansu_core::config::{AuthToken, DbLocation, EnvVars, Settings};
use tansu_core::db::{self, rows, schema};
use tansu_core::model::Id;
use tansu_core::qa::import::import;

/// テストが行を書くデータベースの名前。`create_database` が同じ名前で 2 回呼んでも 1 つにするため、固定の名前で run をまたいで使う。
const DATABASE_NAME: &str = "ci-remote";

/// 環境変数 `name` の値。無ければ、Secrets が渡っていないことが分かるように落とす。
fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} がありません (CI の remote job は Secrets の {name} を渡す)")
    })
}

/// 環境変数の Turso Cloud の DB を指す設定。`TANSU_HOME` は一時ディレクトリにし、実際のホームディレクトリに書かない。
fn remote_settings(home: &tempfile::TempDir) -> Settings {
    let settings = Settings::resolve(EnvVars {
        tansu_home: Some(home.path().to_str().unwrap().into()),
        db_url: Some(required_env("TANSU_DB_URL")),
        db_auth_token: Some(AuthToken::new(required_env("TANSU_DB_AUTH_TOKEN"))),
        ..EnvVars::default()
    })
    .unwrap();
    assert!(
        matches!(settings.db, DbLocation::Remote { .. }),
        "TANSU_DB_URL が libsql:// ではありません"
    );
    settings
}

/// `schema_migration` の版の一覧 (昇順)。
async fn recorded_migrations(connection: &libsql::Connection) -> Vec<i64> {
    let mut rows = connection
        .query("SELECT version FROM schema_migration ORDER BY version", ())
        .await
        .unwrap();
    let mut versions = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        versions.push(row.get::<i64>(0).unwrap());
    }
    versions
}

/// `title=<title>` で絞り込んだ行の ID。
async fn row_ids_with_title(connection: &libsql::Connection, title: &str) -> Vec<String> {
    let (_, found) = rows::query(
        connection,
        DATABASE_NAME,
        &[format!("title={title}")],
        None,
        None,
    )
    .await
    .unwrap();
    found.into_iter().map(|row| row.id.0).collect()
}

#[tokio::test]
#[ignore = "TANSU_DB_URL / TANSU_DB_AUTH_TOKEN が要る。CI の remote job が -- --ignored で実行する"]
async fn remote_open_applies_migrations_once() {
    let home = tempfile::tempdir().unwrap();
    let settings = remote_settings(&home);

    db::open(&settings).await.unwrap();
    let connection = db::open(&settings).await.unwrap();

    let mut rows = connection
        .query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('database', 'property', 'row') ORDER BY name",
            (),
        )
        .await
        .unwrap();
    let mut table_names = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        table_names.push(row.get::<String>(0).unwrap());
    }
    assert_eq!(table_names, ["database", "property", "row"]);
    assert_eq!(recorded_migrations(&connection).await, [1]);
    assert_eq!(db::schema_version(&connection).await.unwrap(), 1);
}

#[tokio::test]
#[ignore = "TANSU_DB_URL / TANSU_DB_AUTH_TOKEN が要る。CI の remote job が -- --ignored で実行する"]
async fn remote_create_upsert_query_delete() {
    let home = tempfile::tempdir().unwrap();
    let connection = db::open(&remote_settings(&home)).await.unwrap();
    // 前の run の消し残りや同時に動く run の行と混ざらないように、行 ID も絞り込む値も run ごとに一意にする
    let row_id = format!("ci-{}", Id::generate().0);
    let first_title = format!("first-{row_id}");
    let second_title = format!("second-{row_id}");

    schema::create_database(&connection, DATABASE_NAME)
        .await
        .unwrap();
    schema::change_schema(&connection, DATABASE_NAME, &["title:text".into()], &[], &[])
        .await
        .unwrap();
    rows::upsert(
        &connection,
        DATABASE_NAME,
        &json!({"id": row_id, "values": {"title": first_title}}),
    )
    .await
    .unwrap();
    assert_eq!(
        row_ids_with_title(&connection, &first_title).await,
        std::slice::from_ref(&row_id)
    );

    rows::upsert(
        &connection,
        DATABASE_NAME,
        &json!({"id": row_id, "values": {"title": second_title}}),
    )
    .await
    .unwrap();
    assert!(
        row_ids_with_title(&connection, &first_title)
            .await
            .is_empty()
    );
    assert_eq!(
        row_ids_with_title(&connection, &second_title).await,
        std::slice::from_ref(&row_id)
    );

    rows::delete(&connection, DATABASE_NAME, &row_id)
        .await
        .unwrap();
    assert!(
        row_ids_with_title(&connection, &second_title)
            .await
            .is_empty()
    );
}

/// `qa import` は QA の 4 つの表への書き込みを 1 つのトランザクションで行う、いちばん重い書き込み。
/// Turso の文書は対話的なトランザクションに 5 秒の期限があると書く
/// (https://docs.turso.tech/sdk/ts/reference「Interactive transactions in libSQL lock the database for writing until
/// committed or rolled back, with a 5-second timeout.」) ため、fixture の取り込みがリモートでコミットまで通るかを確かめ、
/// 所要時間を出力する。取り込みは自然キーで upsert するため、CI 用の DB に残っても次の run で行は増えない。
#[tokio::test]
#[ignore = "TANSU_DB_URL / TANSU_DB_AUTH_TOKEN が要る。CI の remote job が -- --ignored で実行する"]
async fn remote_qa_import_of_the_fixtures() {
    let home = tempfile::tempdir().unwrap();
    let connection = db::open(&remote_settings(&home)).await.unwrap();
    let fixtures =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/qa/sample-notes");

    let started = std::time::Instant::now();
    let report = import(&connection, &[fixtures]).await.unwrap();
    println!("qa import をリモートで {:?} で終えた", started.elapsed());

    assert_eq!(report.counts.project, 1);
    assert!(report.counts.item > 0);
}
