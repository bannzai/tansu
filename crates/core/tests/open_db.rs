use tansu_core::config::{DbLocation, EnvVars, Settings};
use tansu_core::db;

/// `TANSU_HOME` を一時ディレクトリにした設定。実際のホームディレクトリには書かない。
/// 環境変数そのものは書き換えず、解決済みの値を直接渡す (edition 2024 で `set_var` が unsafe で、並列テストと競合するため)。
fn settings_under(home: &tempfile::TempDir) -> Settings {
    Settings::resolve(EnvVars {
        tansu_home: Some(home.path().to_str().unwrap().into()),
        ..EnvVars::default()
    })
    .unwrap()
}

async fn table_names(connection: &libsql::Connection) -> Vec<String> {
    let mut rows = connection
        .query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('database', 'property', 'row') ORDER BY name",
            (),
        )
        .await
        .unwrap();
    let mut names = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        names.push(row.get::<String>(0).unwrap());
    }
    names
}

#[tokio::test]
async fn open_creates_the_three_tables_under_tansu_home() {
    let home = tempfile::tempdir().unwrap();
    let settings = settings_under(&home);
    assert_eq!(
        settings.db,
        DbLocation::Local(home.path().join("tansu.sqlite"))
    );

    let connection = db::open(&settings).await.unwrap();

    assert_eq!(table_names(&connection).await, ["database", "property", "row"]);
    assert_eq!(db::schema_version(&connection).await.unwrap(), 1);
    assert!(home.path().join("tansu.sqlite").exists());
}

#[tokio::test]
async fn open_twice_does_not_reapply_migrations() {
    let home = tempfile::tempdir().unwrap();
    let settings = settings_under(&home);

    db::open(&settings).await.unwrap();
    let connection = db::open(&settings).await.unwrap();

    assert_eq!(table_names(&connection).await, ["database", "property", "row"]);
    assert_eq!(db::schema_version(&connection).await.unwrap(), 1);
}
