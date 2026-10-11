//! `qa import` が `fixtures/qa/sample-notes/` を 4 つの表に取り込み、2 回目で行が増えず、`--dry-run` で何も書かないことを確かめる。
//! `TANSU_HOME` はテストごとの一時ディレクトリ。

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tansu_core::config::{EnvVars, Settings};
use tansu_core::db::{self, rows, schema};
use tansu_core::model::Row;
use tansu_core::named_json;
use tansu_core::qa::import::{ImportCounts, dry_run, import};

/// `fixtures/qa/sample-notes` の絶対パス。
fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/qa/sample-notes")
}

/// `TANSU_HOME` を `home` にした DB を開く。
async fn open(home: &tempfile::TempDir) -> libsql::Connection {
    db::open(
        &Settings::resolve(EnvVars {
            tansu_home: Some(home.path().to_str().unwrap().into()),
            ..EnvVars::default()
        })
        .unwrap(),
    )
    .await
    .unwrap()
}

/// データベース `database` の全行。
async fn all_rows(connection: &libsql::Connection, database: &str) -> Vec<Row> {
    rows::query(connection, database, &[], None, None)
        .await
        .unwrap()
        .1
}

/// データベース `database` で `filters` に一致する行を、CLI の形 (プロパティ名と選択肢の name) の JSON で返す。
async fn query_named(
    connection: &libsql::Connection,
    database: &str,
    filters: &[&str],
) -> Vec<Value> {
    let filters: Vec<String> = filters.iter().map(|filter| (*filter).to_owned()).collect();
    let (database, rows) = rows::query(connection, database, &filters, None, None)
        .await
        .unwrap();
    rows.iter()
        .map(|row| named_json::row_to_json(&database, row))
        .collect()
}

/// `project` の全行の `repo`。
async fn repos(connection: &libsql::Connection) -> Vec<String> {
    query_named(connection, "project", &[])
        .await
        .iter()
        .map(|project| {
            project["values"]["repo"]["value"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

/// 行の JSON の `values` から、`{"type", "value"}` の `value` だけを取り出した JSON。
fn plain_values(row: &Value) -> Value {
    Value::Object(
        row["values"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, value)| (name.clone(), value["value"].clone()))
            .collect(),
    )
}

/// `directory` の下を `destination` へ再帰的に複製する (git の外に fixture を置くため)。
fn copy_dir(directory: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// fixture 全体の件数。
fn fixture_counts() -> ImportCounts {
    ImportCounts {
        project: 1,
        feature: 4,
        item: 12,
        run: 8,
    }
}

#[tokio::test]
async fn import_fills_the_four_databases_from_the_fixtures() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let root = fixture_root().canonicalize().unwrap();

    let report = import(&connection, std::slice::from_ref(&root)).await.unwrap();

    assert!(!report.dry_run);
    assert_eq!(report.counts, fixture_counts());
    assert_eq!(
        report.files,
        [
            root.join("QA.md"),
            root.join("features/note_editor/QA.md"),
            root.join("features/note_list/QA.md"),
            root.join("features/settings/QA.md"),
        ]
        .map(|path| path.display().to_string())
    );
    assert_eq!(report.skipped.len(), 8);

    let databases: Vec<String> = schema::list_databases(&connection)
        .await
        .unwrap()
        .into_iter()
        .map(|database| database.name)
        .collect();
    assert_eq!(databases, ["feature", "item", "project", "run"]);
    let project = schema::get_database(&connection, "project").await.unwrap();
    assert!(project.property("repo").unwrap().unique);
    let run = schema::get_database(&connection, "run").await.unwrap();
    assert!(run.property("run_id").unwrap().unique);

    // project: fixture は tansu のリポジトリの中にあるが、ルートそのものではないため repo はディレクトリ名
    let projects = query_named(&connection, "project", &[]).await;
    assert_eq!(projects.len(), 1);
    assert_eq!(
        plain_values(&projects[0]),
        json!({"repo": "sample-notes", "path": root.display().to_string()})
    );
    let project_id = projects[0]["id"].as_str().unwrap();

    // feature: project への relation と frontmatter の verification
    let features = query_named(&connection, "feature", &[]).await;
    let feature_names: Vec<&str> = features
        .iter()
        .map(|feature| feature["values"]["name"]["value"].as_str().unwrap())
        .collect();
    assert_eq!(
        feature_names,
        ["_root", "note_editor", "note_list", "settings"]
    );
    assert_eq!(
        plain_values(&features[1]),
        json!({"project": [project_id], "name": "note_editor", "verification": "agent-browser, manual"})
    );
    let note_editor_id = features[1]["id"].as_str().unwrap();

    // item: 自動化の 2 列と仕様 ID
    assert_eq!(all_rows(&connection, "item").await.len(), 12);
    let items = query_named(
        &connection,
        "item",
        &[format!("feature={note_editor_id}").as_str()],
    )
    .await;
    assert_eq!(items.len(), 3);
    assert_eq!(
        plain_values(&items[1]),
        json!({
            "feature": [note_editor_id],
            "name": "画像を貼れる",
            "expected": "画像をドロップすると本文に埋め込まれる",
            "automation": "manual",
            "automation_detail": "ドロップ操作を E2E で再現できないため",
        })
    );
    let image_item_id = items[1]["id"].as_str().unwrap();
    let pinned = query_named(&connection, "item", &["name=ピン留めの固定"]).await;
    assert_eq!(pinned[0]["values"]["spec_ids"]["value"], json!(["S4"]));
    assert_eq!(pinned[0]["values"]["automation"]["value"], "todo");
    assert!(pinned[0]["values"].get("automation_detail").is_none());
    let login = query_named(&connection, "item", &["name=ログインできる"]).await;
    assert!(login[0]["values"].get("automation").is_none());

    // run: ❌ は ng、⏭️ は skip、チェック済みは ok。確認日・画像・frontmatter の commit を持つ
    assert_eq!(all_rows(&connection, "run").await.len(), 8);
    let image_runs = query_named(
        &connection,
        "run",
        &[format!("item={image_item_id}").as_str()],
    )
    .await;
    assert_eq!(image_runs.len(), 1);
    assert_eq!(
        plain_values(&image_runs[0]),
        json!({
            "run_id": format!("import:{image_item_id}"),
            "item": [image_item_id],
            "result": "ng",
            "note": "ドロップ後に本文が空になる。再現手順: 画像を本文へドロップ → 保存。issue: https://example.com/sample-notes/issues/21",
            "recorded_at": "2026-10-07",
            "source": "import",
            "image_url": "https://example.com/sample-notes/note_editor/image-drop-failed.png",
            "commit": "a1b2c3d4e5f60718293a4b5c6d7e8f90ab12cd34",
        })
    );
    let voice_item_id = items[2]["id"].as_str().unwrap().to_owned();
    let voice_runs = query_named(
        &connection,
        "run",
        &[format!("item={voice_item_id}").as_str()],
    )
    .await;
    // 確認日が無い記録は frontmatter の last_verified_at を recorded_at にし、画像は持たない
    assert_eq!(voice_runs[0]["values"]["result"]["value"], "skip");
    assert_eq!(
        voice_runs[0]["values"]["recorded_at"]["value"],
        "2026-10-07"
    );
    assert!(voice_runs[0]["values"].get("image_url").is_none());
    assert!(
        voice_runs[0]["values"]["note"]["value"]
            .as_str()
            .unwrap()
            .contains("起動条件: ローカル sim-boot")
    );
    // last_verified_* が null の feature の記録は commit も recorded_at も持たない (root の ⏭️)
    let skipped_login = query_named(&connection, "run", &["result=skip"]).await;
    let login_run = skipped_login
        .iter()
        .find(|run| run["values"]["note"]["value"] == "テストアカウントの発行待ち")
        .unwrap();
    assert!(login_run["values"].get("recorded_at").is_none());
    assert!(login_run["values"].get("commit").is_none());
    assert_eq!(
        query_named(&connection, "run", &["result=ok"]).await.len(),
        5
    );
    assert_eq!(
        query_named(&connection, "run", &["result=ng"]).await.len(),
        1
    );
    assert_eq!(
        query_named(&connection, "run", &["result=skip"])
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn importing_the_same_fixtures_twice_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let root = fixture_root();

    let first = import(&connection, std::slice::from_ref(&root)).await.unwrap();
    let mut before = Vec::new();
    for database in ["project", "feature", "item", "run"] {
        before.push(all_rows(&connection, database).await);
    }
    // updated_at は秒の精度なので、同じ秒の中では変わったかを判別できない
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let second = import(&connection, std::slice::from_ref(&root)).await.unwrap();

    assert_eq!(second, first);
    let mut after = Vec::new();
    for database in ["project", "feature", "item", "run"] {
        after.push(all_rows(&connection, database).await);
    }
    assert_eq!(after, before);
    assert_eq!(
        after.iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 4, 12, 8]
    );
}

#[test]
fn dry_run_reports_the_same_counts_without_a_connection() {
    let report = dry_run(&[fixture_root()]).unwrap();

    assert!(report.dry_run);
    assert_eq!(report.counts, fixture_counts());
    assert_eq!(report.skipped.len(), 8);
    assert!(
        dry_run(&[PathBuf::from("/nonexistent/QA.md")])
            .unwrap_err()
            .to_string()
            .contains("/nonexistent/QA.md")
    );
}

#[tokio::test]
async fn project_repo_comes_from_the_git_remote_of_the_root() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("git が無いため飛ばす");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let repository = tempfile::tempdir().unwrap();
    let root = repository.path().join("notes");
    copy_dir(&fixture_root(), &root);
    for args in [
        vec!["init", "-q"],
        vec!["remote", "add", "origin", "git@github.com:acme/notes.git"],
    ] {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    import(&connection, std::slice::from_ref(&root)).await.unwrap();

    assert_eq!(repos(&connection).await, ["acme/notes"]);

    // ルートの下のディレクトリを指定した時は、囲むリポジトリの名前ではなくそのディレクトリの名前
    import(&connection, &[root.join("features")]).await.unwrap();
    assert_eq!(repos(&connection).await, ["acme/notes", "features"]);
}

#[tokio::test]
async fn a_single_file_outside_git_belongs_to_its_directory() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("QA.md");
    std::fs::copy(fixture_root().join("features/note_editor/QA.md"), &file).unwrap();

    let report = import(&connection, std::slice::from_ref(&file)).await.unwrap();

    assert_eq!(
        report.counts,
        ImportCounts {
            project: 1,
            feature: 1,
            item: 3,
            run: 3
        }
    );
    let projects = query_named(&connection, "project", &[]).await;
    assert_eq!(
        projects[0]["values"]["repo"]["value"],
        directory.path().file_name().unwrap().to_str().unwrap()
    );
    assert_eq!(
        projects[0]["values"]["path"]["value"],
        directory
            .path()
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );

    // 同じファイルをもう 1 度取り込んでも行は増えない
    assert_eq!(
        import(&connection, &[file]).await.unwrap().counts,
        report.counts
    );
    assert_eq!(all_rows(&connection, "feature").await.len(), 1);
    assert_eq!(all_rows(&connection, "item").await.len(), 3);
    assert_eq!(all_rows(&connection, "run").await.len(), 3);
}

#[tokio::test]
async fn missing_path_is_an_error_and_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;

    let error = import(
        &connection,
        &[fixture_root(), PathBuf::from("/nonexistent/QA.md")],
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("/nonexistent/QA.md"), "{error}");
    assert!(
        schema::list_databases(&connection)
            .await
            .unwrap()
            .is_empty()
    );
}
