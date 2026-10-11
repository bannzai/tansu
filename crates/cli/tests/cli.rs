//! `tansu` のバイナリを別プロセスとして実行し、CLI の入出力と DB への保存を確かめる。
//! どのテストも `TANSU_HOME` と `HOME` をテストごとの一時ディレクトリにし、実際のホームディレクトリには書かない。

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::{Value, json};
use tansu_core::config::{EnvVars, Settings};
use tempfile::TempDir;

/// テスト 1 つ分の置き場所。
struct Sandbox {
    /// `TANSU_HOME` にするディレクトリ。DB はこの下にできる。
    tansu_home: TempDir,
    /// `HOME` にするディレクトリ。tansu がここに何も書かないことを確かめる。
    user_home: TempDir,
}

/// 空の一時ディレクトリ 2 つで置き場所を作る。
fn sandbox() -> Sandbox {
    Sandbox {
        tansu_home: tempfile::tempdir().unwrap(),
        user_home: tempfile::tempdir().unwrap(),
    }
}

/// `sandbox` を `TANSU_HOME` と `HOME` にし、DB の接続先の環境変数を外した `tansu <args>` のコマンド。
fn command(sandbox: &Sandbox, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tansu"));
    command
        .args(args)
        .env("TANSU_HOME", sandbox.tansu_home.path())
        .env("HOME", sandbox.user_home.path())
        .env_remove("TANSU_DB_URL")
        .env_remove("TANSU_DB_AUTH_TOKEN");
    command
}

/// 成功したことを確かめ、stdout を返す。
fn succeed(sandbox: &Sandbox, args: &[&str]) -> String {
    let output = command(sandbox, args).output().unwrap();
    assert!(
        output.status.success(),
        "tansu {args:?} が失敗した: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// 成功したことを確かめ、stdout を JSON として返す。
fn succeed_json(sandbox: &Sandbox, args: &[&str]) -> Value {
    serde_json::from_str(&succeed(sandbox, args)).unwrap()
}

/// exit 1 で終わり、stdout が空で stderr が 1 行であることを確かめ、その 1 行を返す。
fn assert_failed(output: Output) -> String {
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(output.stdout.is_empty());
    assert_eq!(stderr.lines().count(), 1, "stderr: {stderr}");
    stderr
}

/// `tansu <args>` が失敗したことを [`assert_failed`] で確かめ、stderr の 1 行を返す。
fn fail(sandbox: &Sandbox, args: &[&str]) -> String {
    assert_failed(command(sandbox, args).output().unwrap())
}

/// `tansu upsert <database> --json <row>` を成功させ、書いた行の JSON を返す。
fn upsert(sandbox: &Sandbox, database: &str, row: Value) -> Value {
    succeed_json(sandbox, &["upsert", database, "--json", &row.to_string()])
}

/// `tansu query <database> --json <options>` を成功させ、行の JSON の配列を返す。
fn query(sandbox: &Sandbox, database: &str, options: &[&str]) -> Vec<Value> {
    let mut args = vec!["query", database, "--json"];
    args.extend(options);
    succeed_json(sandbox, &args).as_array().unwrap().clone()
}

/// 行の JSON の `id` を並びのまま返す。
fn row_ids(rows: &[Value]) -> Vec<&str> {
    rows.iter().map(|row| row["id"].as_str().unwrap()).collect()
}

/// select / multi_select のプロパティの定義の JSON から、選択肢の name を並びのまま返す。
fn option_names(property: &Value) -> Vec<&str> {
    property["kind"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option["name"].as_str().unwrap())
        .collect()
}

#[test]
fn schema_with_all_eight_kinds_persists_across_processes() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "projects"]);
    succeed(&sandbox, &["db", "create", "tasks"]);
    let mut args = vec!["db", "schema", "tasks"];
    for spec in [
        "title:text",
        "points:number",
        "done:checkbox",
        "link:url",
        "due:date",
        "status:select:todo,done",
        "labels:multi_select:bug,ui",
        "project:relation:projects",
    ] {
        args.extend(["--add", spec]);
    }
    let created = succeed_json(&sandbox, &args);

    let reread = succeed_json(&sandbox, &["db", "schema", "tasks"]);
    assert_eq!(reread, created);
    let kinds: Vec<(&str, &str)> = reread["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|property| {
            (
                property["name"].as_str().unwrap(),
                property["kind"]["type"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("title", "text"),
            ("points", "number"),
            ("done", "checkbox"),
            ("link", "url"),
            ("due", "date"),
            ("status", "select"),
            ("labels", "multi_select"),
            ("project", "relation"),
        ]
    );
    assert_eq!(option_names(&reread["properties"][5]), ["todo", "done"]);
    assert_eq!(option_names(&reread["properties"][6]), ["bug", "ui"]);
    assert_eq!(
        reread["properties"][7]["kind"]["target_database_id"],
        succeed_json(&sandbox, &["db", "schema", "projects"])["id"]
    );
    let listed = succeed_json(&sandbox, &["db", "list"]);
    let listed_names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|database| database["name"].as_str().unwrap())
        .collect();
    assert_eq!(listed_names, ["projects", "tasks"]);
    assert_eq!(succeed_json(&sandbox, &["db", "create", "tasks"]), created);

    // 同じ指定を再実行しても定義は変わらない
    assert_eq!(succeed_json(&sandbox, &args), created);
    // select には無い選択肢だけを足す
    let merged = succeed_json(
        &sandbox,
        &[
            "db",
            "schema",
            "tasks",
            "--add",
            "status:select:todo,done,later",
        ],
    );
    assert_eq!(
        option_names(&merged["properties"][5]),
        ["todo", "done", "later"]
    );
    assert_eq!(
        merged["properties"][5]["kind"]["options"][0]["id"],
        created["properties"][5]["kind"]["options"][0]["id"]
    );
    fail(
        &sandbox,
        &["db", "schema", "tasks", "--add", "title:number"],
    );
    fail(&sandbox, &["db", "schema", "tasks", "--add", "size:huge"]);

    // 外したプロパティは定義からも行の値からも消える
    let written = upsert(
        &sandbox,
        "tasks",
        json!({"id": "t1", "values": {"title": "a", "points": 1}}),
    );
    // updated_at は秒の精度なので、同じ秒の中では変わったかを判別できない
    std::thread::sleep(Duration::from_millis(1100));
    let removed = succeed_json(&sandbox, &["db", "schema", "tasks", "--remove", "points"]);
    assert!(
        removed["properties"]
            .as_array()
            .unwrap()
            .iter()
            .all(|property| property["name"] != "points")
    );
    let after_remove = query(&sandbox, "tasks", &[])[0].clone();
    assert_eq!(
        after_remove["values"],
        json!({"title": {"type": "text", "value": "a"}})
    );
    assert_ne!(after_remove["updated_at"], written["updated_at"]);
    assert_eq!(
        succeed_json(&sandbox, &["db", "schema", "tasks", "--remove", "points"]),
        removed
    );
}

#[test]
fn upsert_query_delete_export() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "tasks"]);
    succeed(
        &sandbox,
        &[
            "db",
            "schema",
            "tasks",
            "--add",
            "title:text",
            "--add",
            "points:number",
            "--add",
            "status:select:todo,done",
        ],
    );
    let task = json!({
        "id": "task-1",
        "body": "手順の本文",
        "values": {"title": "Write docs", "points": 3, "status": "todo"},
    });
    let first = upsert(&sandbox, "tasks", task.clone());
    assert_eq!(first["id"], "task-1");
    // updated_at は秒の精度なので、同じ秒の中の 2 回目では変わったかを判別できない
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(upsert(&sandbox, "tasks", task), first);
    assert_eq!(query(&sandbox, "tasks", &[]).len(), 1);

    let mut child = command(&sandbox, &["upsert", "tasks"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"id": "task-2", "values": {"title": "Fix bug", "points": 5, "status": "done"}})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());

    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &[])),
        ["task-1", "task-2"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--where", "status=done"])),
        ["task-2"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--where", "title^=Write"])),
        ["task-1"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--where", "points=5"])),
        ["task-2"]
    );
    assert!(
        query(
            &sandbox,
            "tasks",
            &["--where", "status=todo", "--where", "points=5"]
        )
        .is_empty()
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--sort", "points:desc"])),
        ["task-2", "task-1"]
    );
    assert_eq!(
        row_ids(&query(
            &sandbox,
            "tasks",
            &["--sort", "points:desc", "--limit", "1"]
        )),
        ["task-2"]
    );
    fail(
        &sandbox,
        &["query", "tasks", "--json", "--where", "status=later"],
    );
    fail(
        &sandbox,
        &["query", "tasks", "--json", "--where", "points^=5"],
    );
    fail(&sandbox, &["query", "missing", "--json"]);
    fail(&sandbox, &["query", "tasks"]);

    let markdown = succeed(&sandbox, &["export", "tasks", "--format", "markdown"]);
    assert!(markdown.starts_with("# tasks\n"), "{markdown}");
    assert!(
        markdown.contains("| id | title | points | status |"),
        "{markdown}"
    );
    assert!(
        markdown.contains("| task-1 | Write docs | 3 | todo |"),
        "{markdown}"
    );
    assert!(markdown.contains("## task-1\n\n手順の本文"), "{markdown}");
    let exported = succeed_json(&sandbox, &["export", "tasks", "--format", "json"]);
    assert_eq!(exported["database"]["name"], "tasks");
    assert_eq!(exported["rows"].as_array().unwrap().len(), 2);
    assert_eq!(
        exported["rows"][1]["values"]["status"],
        json!({"type": "select", "value": "done"})
    );

    assert_eq!(succeed(&sandbox, &["delete", "tasks", "task-1"]), "");
    assert_eq!(row_ids(&query(&sandbox, "tasks", &[])), ["task-2"]);
    assert_eq!(succeed(&sandbox, &["delete", "tasks", "task-1"]), "");

    // values に書いたプロパティだけを変え、null は値を消す
    let updated = upsert(
        &sandbox,
        "tasks",
        json!({"id": "task-2", "values": {"points": null}}),
    );
    assert_eq!(
        updated["values"],
        json!({
            "title": {"type": "text", "value": "Fix bug"},
            "status": {"type": "select", "value": "done"},
        })
    );
}

#[test]
fn upsert_without_key_is_rejected() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "tasks"]);
    succeed(&sandbox, &["db", "schema", "tasks", "--add", "title:text"]);
    let stderr = fail(
        &sandbox,
        &["upsert", "tasks", "--json", r#"{"values": {"title": "a"}}"#],
    );
    assert!(stderr.contains("`id`"), "{stderr}");
    assert!(query(&sandbox, "tasks", &[]).is_empty());
}

#[test]
fn unique_property_identifies_rows_and_rejects_duplicates() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "projects"]);
    succeed(
        &sandbox,
        &[
            "db",
            "schema",
            "projects",
            "--add",
            "repo:text",
            "--add",
            "path:text",
        ],
    );
    upsert(
        &sandbox,
        "projects",
        json!({"id": "p1", "values": {"repo": "owner/a"}}),
    );
    upsert(
        &sandbox,
        "projects",
        json!({"id": "p2", "values": {"repo": "owner/a"}}),
    );
    let stderr = fail(&sandbox, &["db", "schema", "projects", "--unique", "repo"]);
    assert!(stderr.contains("repo"), "{stderr}");
    succeed(&sandbox, &["delete", "projects", "p2"]);
    let schema = succeed_json(&sandbox, &["db", "schema", "projects", "--unique", "repo"]);
    assert_eq!(schema["properties"][0]["unique"], true);

    let by_repo = json!({"values": {"repo": "owner/a", "path": "/src/a"}});
    assert_eq!(upsert(&sandbox, "projects", by_repo.clone())["id"], "p1");
    assert_eq!(upsert(&sandbox, "projects", by_repo)["id"], "p1");
    let new_repo = json!({"values": {"repo": "owner/b"}});
    let created = upsert(&sandbox, "projects", new_repo.clone());
    assert_eq!(created["id"].as_str().unwrap().len(), 21);
    assert_eq!(upsert(&sandbox, "projects", new_repo)["id"], created["id"]);
    let rows = query(&sandbox, "projects", &[]);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0]["values"]["path"],
        json!({"type": "text", "value": "/src/a"})
    );

    let stderr = fail(
        &sandbox,
        &[
            "upsert",
            "projects",
            "--json",
            r#"{"id": "p3", "values": {"repo": "owner/a"}}"#,
        ],
    );
    assert!(stderr.contains("unique"), "{stderr}");
    assert_eq!(query(&sandbox, "projects", &[]).len(), 2);
}

#[test]
fn relation_reference_is_removed_when_target_row_is_deleted() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "projects"]);
    succeed(&sandbox, &["db", "create", "tasks"]);
    succeed(
        &sandbox,
        &[
            "db",
            "schema",
            "tasks",
            "--add",
            "project:relation:projects",
        ],
    );
    upsert(&sandbox, "projects", json!({"id": "p1"}));
    upsert(
        &sandbox,
        "tasks",
        json!({"id": "t1", "values": {"project": ["p1"]}}),
    );

    assert_eq!(
        query(&sandbox, "tasks", &[])[0]["values"]["project"],
        json!({"type": "relation", "value": ["p1"]})
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--where", "project=p1"])),
        ["t1"]
    );
    fail(
        &sandbox,
        &[
            "upsert",
            "tasks",
            "--json",
            r#"{"id": "t2", "values": {"project": ["missing"]}}"#,
        ],
    );

    succeed(&sandbox, &["delete", "projects", "p1"]);
    assert_eq!(query(&sandbox, "tasks", &[])[0]["values"], json!({}));
}

#[test]
fn delete_is_rolled_back_when_removing_references_breaks_unique() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "projects"]);
    succeed(&sandbox, &["db", "create", "tasks"]);
    succeed(
        &sandbox,
        &[
            "db",
            "schema",
            "tasks",
            "--add",
            "project:relation:projects",
            "--unique",
            "project",
        ],
    );
    upsert(&sandbox, "projects", json!({"id": "p1"}));
    upsert(&sandbox, "projects", json!({"id": "p2"}));
    upsert(
        &sandbox,
        "tasks",
        json!({"id": "t1", "values": {"project": ["p1", "p2"]}}),
    );
    upsert(
        &sandbox,
        "tasks",
        json!({"id": "t2", "values": {"project": ["p2"]}}),
    );

    // p1 を外すと t1 と t2 がどちらも ["p2"] になり unique が破れるため、削除ごと失敗する
    let stderr = fail(&sandbox, &["delete", "projects", "p1"]);
    assert!(stderr.contains("unique"), "{stderr}");
    assert_eq!(row_ids(&query(&sandbox, "projects", &[])), ["p1", "p2"]);
    assert_eq!(
        query(&sandbox, "tasks", &[])[0]["values"]["project"],
        json!({"type": "relation", "value": ["p1", "p2"]})
    );
}

#[test]
fn date_sort_compares_instants_across_offsets() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "events"]);
    succeed(&sandbox, &["db", "schema", "events", "--add", "at:date"]);
    // 文字列の順は day < utc < tokyo だが、時刻の順は day (10 日 0 時 UTC) < tokyo (11 日 0 時 UTC) < utc (11 日 1 時 UTC)
    for (id, at) in [
        ("tokyo", "2026-10-11T09:00:00+09:00"),
        ("utc", "2026-10-11T01:00:00Z"),
        ("day", "2026-10-10"),
    ] {
        upsert(&sandbox, "events", json!({"id": id, "values": {"at": at}}));
    }
    assert_eq!(
        row_ids(&query(&sandbox, "events", &["--sort", "at"])),
        ["day", "tokyo", "utc"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "events", &["--sort", "at:desc"])),
        ["utc", "tokyo", "day"]
    );
    // 区切りの t / z を小文字で書いても、保存時と同じく大文字にして検索する
    upsert(
        &sandbox,
        "events",
        json!({"id": "lower", "values": {"at": "2026-10-12t00:00:00z"}}),
    );
    assert_eq!(
        row_ids(&query(
            &sandbox,
            "events",
            &["--where", "at=2026-10-12t00:00:00z"]
        )),
        ["lower"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "events", &["--where", "at^=2026-10-11t"])),
        ["tokyo", "utc"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "events", &["--sort", "at:desc", "--limit", "1"])),
        ["lower"]
    );
}

#[tokio::test]
async fn select_is_name_in_cli_and_option_id_in_db() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "tasks"]);
    let schema = succeed_json(
        &sandbox,
        &[
            "db",
            "schema",
            "tasks",
            "--add",
            "status:select:todo,done",
            "--add",
            "labels:multi_select:bug,ui",
        ],
    );
    let done_id = schema["properties"][0]["kind"]["options"][1]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let bug_id = schema["properties"][1]["kind"]["options"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let row = upsert(
        &sandbox,
        "tasks",
        json!({"id": "t1", "values": {"status": "done", "labels": ["bug"]}}),
    );
    assert_eq!(
        row["values"],
        json!({
            "status": {"type": "select", "value": "done"},
            "labels": {"type": "multi_select", "value": ["bug"]},
        })
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--where", "labels=bug"])),
        ["t1"]
    );

    let settings = Settings::resolve(EnvVars {
        tansu_home: Some(sandbox.tansu_home.path().to_str().unwrap().into()),
        ..EnvVars::default()
    })
    .unwrap();
    let connection = tansu_core::db::open(&settings).await.unwrap();
    let mut rows = connection
        .query(r#"SELECT "values" FROM "row""#, ())
        .await
        .unwrap();
    let stored = rows
        .next()
        .await
        .unwrap()
        .unwrap()
        .get::<String>(0)
        .unwrap();
    assert!(stored.contains(&done_id), "{stored}");
    assert!(stored.contains(&bug_id), "{stored}");
    assert!(!stored.contains(r#""done""#), "{stored}");
    assert!(!stored.contains(r#""bug""#), "{stored}");
}

#[test]
fn writes_only_under_tansu_home() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "tasks"]);
    succeed(&sandbox, &["db", "schema", "tasks", "--add", "title:text"]);
    upsert(
        &sandbox,
        "tasks",
        json!({"id": "t1", "values": {"title": "a"}}),
    );
    query(&sandbox, "tasks", &[]);
    succeed(&sandbox, &["export", "tasks", "--format", "markdown"]);

    assert_eq!(
        std::fs::read_dir(sandbox.user_home.path()).unwrap().count(),
        0
    );
    assert!(sandbox.tansu_home.path().join("tansu.sqlite").exists());
}

#[test]
fn libsql_url_is_not_supported_yet() {
    let sandbox = sandbox();
    let stderr = assert_failed(
        command(&sandbox, &["db", "list"])
            .env("TANSU_DB_URL", "libsql://example.turso.io")
            .env("TANSU_DB_AUTH_TOKEN", "token")
            .output()
            .unwrap(),
    );
    assert!(stderr.contains("libsql://"), "{stderr}");
}
