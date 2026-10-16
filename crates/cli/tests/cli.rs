//! `tansu` のバイナリを別プロセスとして実行し、CLI の入出力と DB への保存を確かめる。
//! どのテストも `TANSU_HOME` と `HOME` をテストごとの一時ディレクトリにし、実際のホームディレクトリには書かない。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
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
    // --where で名前を切り出せない名前は作れない
    fail(&sandbox, &["db", "schema", "tasks", "--add", "a=b:text"]);
    fail(&sandbox, &["db", "schema", "tasks", "--add", "a^:text"]);
    fail(&sandbox, &["db", "schema", "tasks", "--add", "a<b:text"]);
    fail(&sandbox, &["db", "schema", "tasks", "--add", "a>b:text"]);

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
        markdown.contains("| `task-1` | Write docs | 3 | todo |"),
        "{markdown}"
    );
    assert!(markdown.contains("## `task-1`\n\n手順の本文"), "{markdown}");
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

    // body は書いた時だけ置き換え、省略と null では残す
    upsert(
        &sandbox,
        "tasks",
        json!({"id": "task-2", "body": "再現手順"}),
    );
    assert_eq!(
        upsert(&sandbox, "tasks", json!({"id": "task-2", "values": {}}))["body"],
        "再現手順"
    );
    assert_eq!(
        upsert(&sandbox, "tasks", json!({"id": "task-2", "body": null}))["body"],
        "再現手順"
    );
    assert_eq!(
        upsert(&sandbox, "tasks", json!({"id": "task-2", "body": ""}))["body"],
        ""
    );

    // delete の後の書き出しには残った行だけが出る
    let exported = succeed_json(&sandbox, &["export", "tasks", "--format", "json"]);
    assert_eq!(row_ids(exported["rows"].as_array().unwrap()), ["task-2"]);

    // 並べ替えの向きは大文字でもよく、それ以外の指定はエラー
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--sort", "title:DESC"])),
        ["task-2"]
    );
    let stderr = fail(
        &sandbox,
        &["query", "tasks", "--json", "--sort", "title:up"],
    );
    assert!(stderr.contains("up"), "{stderr}");
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
    // markdown の書き出しでも、relation の値の行 ID は文字列のまま (コードスパン) 出る
    let markdown = succeed(&sandbox, &["export", "tasks", "--format", "markdown"]);
    assert!(markdown.contains("| `t1` | `p1` |"), "{markdown}");

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
    // 並びが違うだけの値は同じ値とみなす
    let stderr = fail(
        &sandbox,
        &[
            "upsert",
            "tasks",
            "--json",
            r#"{"id": "t3", "values": {"project": ["p2", "p1"]}}"#,
        ],
    );
    assert!(stderr.contains("unique"), "{stderr}");

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
fn sort_puts_rows_without_value_last_and_checkbox_filters() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "tasks"]);
    succeed(
        &sandbox,
        &[
            "db",
            "schema",
            "tasks",
            "--add",
            "points:number",
            "--add",
            "done:checkbox",
        ],
    );
    for row in [
        json!({"id": "a", "values": {"points": 2, "done": true}}),
        json!({"id": "b", "values": {"done": false}}),
        json!({"id": "c", "values": {"points": 1}}),
    ] {
        upsert(&sandbox, "tasks", row);
    }
    // 値の無い b は昇順でも降順でも最後
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--sort", "points"])),
        ["c", "a", "b"]
    );
    assert_eq!(
        row_ids(&query(
            &sandbox,
            "tasks",
            &["--sort", "points", "--limit", "1"]
        )),
        ["c"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--sort", "points:desc"])),
        ["a", "c", "b"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--where", "done=true"])),
        ["a"]
    );
    assert_eq!(
        row_ids(&query(&sandbox, "tasks", &["--where", "done=false"])),
        ["b"]
    );
    fail(
        &sandbox,
        &["query", "tasks", "--json", "--where", "done=yes"],
    );

    // 別のデータベースの行の ID では書けず、消せない
    succeed(&sandbox, &["db", "create", "others"]);
    let stderr = fail(&sandbox, &["upsert", "others", "--json", r#"{"id": "a"}"#]);
    assert!(stderr.contains("別のデータベース"), "{stderr}");
    let stderr = fail(&sandbox, &["delete", "others", "a"]);
    assert!(stderr.contains("別のデータベース"), "{stderr}");
    assert_eq!(row_ids(&query(&sandbox, "tasks", &["--limit", "1"])), ["a"]);
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
        row_ids(&query(
            &sandbox,
            "events",
            &["--sort", "at:desc", "--limit", "1"]
        )),
        ["lower"]
    );
}

#[test]
fn number_comparisons_include_or_exclude_the_boundary() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "tasks"]);
    succeed(
        &sandbox,
        &[
            "db",
            "schema",
            "tasks",
            "--add",
            "points:number",
            "--add",
            "title:text",
            "--add",
            "done:checkbox",
            "--add",
            "status:select:todo,done",
        ],
    );
    for (id, values) in [
        ("one", json!({"points": 1})),
        ("two", json!({"points": 2})),
        ("half", json!({"points": 2.5})),
        ("none", json!({"title": "値なし"})),
    ] {
        upsert(&sandbox, "tasks", json!({"id": id, "values": values}));
    }
    // 値の無い none はどの比較にも一致しない
    for (filter, expected) in [
        ("points>=2", vec!["two", "half"]),
        ("points<=2", vec!["one", "two"]),
        ("points>2", vec!["half"]),
        ("points<2", vec!["one"]),
        ("points>=2.5", vec!["half"]),
        ("points<1", vec![]),
        ("points<100", vec!["one", "two", "half"]),
    ] {
        assert_eq!(
            row_ids(&query(&sandbox, "tasks", &["--where", filter])),
            expected,
            "{filter}"
        );
    }
    // 複数の比較は AND
    assert_eq!(
        row_ids(&query(
            &sandbox,
            "tasks",
            &["--where", "points>1", "--where", "points<2.5"]
        )),
        ["two"]
    );
    // number / date 以外の型の比較と、数値でない値はエラー
    for filter in ["title>=a", "done>true", "status<done"] {
        let stderr = fail(&sandbox, &["query", "tasks", "--json", "--where", filter]);
        assert!(stderr.contains("number / date"), "{stderr}");
    }
    let stderr = fail(
        &sandbox,
        &["query", "tasks", "--json", "--where", "points>=abc"],
    );
    assert!(stderr.contains("数値"), "{stderr}");
}

#[test]
fn recorded_at_comparison_counts_runs_since_a_date() {
    let sandbox = sandbox();
    let fixtures = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/qa/sample-notes"
    );
    // qa import の run (source=import、日付だけの recorded_at) は 2026-10-05 〜 08 にある
    succeed(&sandbox, &["qa", "import", fixtures]);
    // 日付だけの値はその日の 0 時 UTC、時差付きの値は UTC に直した時刻で比べる
    for (id, at) in [
        ("day-before", json!("2026-10-06")),
        ("day", json!("2026-10-07")),
        ("tokyo-before", json!("2026-10-07T08:59:59+09:00")),
        ("tokyo-at", json!("2026-10-07T09:00:00+09:00")),
        ("utc-after", json!("2026-10-08T12:00:00Z")),
        ("no-date", Value::Null),
    ] {
        upsert(
            &sandbox,
            "run",
            json!({"id": id, "values": {"run_id": id, "source": "record", "recorded_at": at}}),
        );
    }
    // documents/DIRECTION.md の判定基準の計測コマンド (`--where "recorded_at>=<日付>" --where source=record`) の形。
    // source=record で qa import の run を除く
    for (condition, expected) in [
        (">=2026-10-07", vec!["day", "tokyo-at", "utc-after"]),
        (">2026-10-07", vec!["utc-after"]),
        ("<2026-10-07", vec!["day-before", "tokyo-before"]),
        (
            "<=2026-10-07T00:00:00Z",
            vec!["day-before", "day", "tokyo-before", "tokyo-at"],
        ),
        (">=2026-10-07T00:00:00.001Z", vec!["utc-after"]),
    ] {
        let filter = format!("recorded_at{condition}");
        assert_eq!(
            row_ids(&query(
                &sandbox,
                "run",
                &["--where", &filter, "--where", "source=record"]
            )),
            expected,
            "{filter}"
        );
    }
    // 日付として読めない値は比べる時刻が決まらないのでエラー
    let stderr = fail(
        &sandbox,
        &["query", "run", "--json", "--where", "recorded_at>=2026-10"],
    );
    assert!(stderr.contains("日付"), "{stderr}");
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
fn qa_import_dry_run_writes_nothing_and_import_is_idempotent() {
    let sandbox = sandbox();
    let fixtures = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/qa/sample-notes"
    );

    let dry = succeed_json(&sandbox, &["qa", "import", "--dry-run", "--json", fixtures]);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(
        dry["counts"],
        json!({"project": 1, "feature": 4, "item": 12, "run": 8})
    );
    assert_eq!(dry["files"].as_array().unwrap().len(), 4);
    assert_eq!(dry["skipped"].as_array().unwrap().len(), 8);
    assert!(
        dry["skipped"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with("features/note_list/QA.md"),
        "{}",
        dry["skipped"][0]
    );
    assert_eq!(dry["skipped"][0]["line"], 34);
    assert_eq!(dry["skipped"][0]["content"], "  - 優先度: 高");
    assert!(dry["skipped"][0]["reason"].is_string());
    // --dry-run は DB を開かず、ファイルも作らない
    assert!(!sandbox.tansu_home.path().join("tansu.sqlite").exists());
    assert_eq!(succeed_json(&sandbox, &["db", "list"]), json!([]));

    let imported = succeed_json(&sandbox, &["qa", "import", "--json", fixtures]);
    assert_eq!(imported["dry_run"], false);
    assert_eq!(imported["counts"], dry["counts"]);
    assert_eq!(imported["skipped"], dry["skipped"]);
    assert_eq!(
        query(&sandbox, "run", &["--where", "source=import"]).len(),
        8
    );
    assert_eq!(row_ids(&query(&sandbox, "project", &[])).len(), 1);

    // 2 回目も同じ出力で、行は増えない
    assert_eq!(
        succeed_json(&sandbox, &["qa", "import", "--json", fixtures]),
        imported
    );
    assert_eq!(query(&sandbox, "feature", &[]).len(), 4);
    assert_eq!(query(&sandbox, "item", &[]).len(), 12);
    assert_eq!(query(&sandbox, "run", &[]).len(), 8);

    let text = succeed(&sandbox, &["qa", "import", fixtures]);
    assert!(
        text.starts_with(
            "project: 1, feature: 4, item: 12, run: 8 (4 ファイル)\n読み飛ばした行: 8 行\n"
        ),
        "{text}"
    );
    let stderr = fail(&sandbox, &["qa", "import", "--json", "/nonexistent/QA.md"]);
    assert!(stderr.contains("/nonexistent/QA.md"), "{stderr}");
}

/// `root` の下の `relative` に合成の QA.md を書く (途中のディレクトリも作る)。
fn write_qa_md(root: &Path, relative: &str) {
    let file = root.join(relative);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, "# QA\n\n- [ ] **一覧が開く**: 項目が並ぶ\n").unwrap();
}

/// `qa import <root> --dry-run --json` の `files` を、`root` からの相対パスにして返す。
fn dry_run_files(sandbox: &Sandbox, root: &Path) -> Vec<String> {
    let report = succeed_json(
        sandbox,
        &[
            "qa",
            "import",
            "--dry-run",
            "--json",
            root.to_str().unwrap(),
        ],
    );
    let root = root.canonicalize().unwrap();
    let mut files: Vec<String> = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            Path::new(file.as_str().unwrap())
                .strip_prefix(&root)
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    files.sort();
    files
}

// symlink を作るため unix に限る (CI は ubuntu)
#[cfg(unix)]
#[test]
fn qa_import_skips_nested_repositories_and_gitignored_paths_in_a_git_work_tree() {
    let sandbox = sandbox();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("notes");
    write_qa_md(&root, "QA.md");
    write_qa_md(&root, "features/removed/QA.md");
    write_qa_md(&root, "features/linked/QA.md");
    git(&root, &["init", "-q"]);
    commit_all(&root, "first");
    // agent が作った作業用の worktree (ルートに `.git` のファイルを持つ。commit 済みの QA.md の複製が入る)
    git(&root, &["worktree", "add", "-q", ".claude/worktrees/agent"]);
    // 削除してまだ commit していない QA.md (git の index には残る)
    std::fs::remove_file(root.join("features/removed/QA.md")).unwrap();
    // commit 済みのディレクトリを、index を更新せずにリポジトリの外への symlink に置き換える
    write_qa_md(directory.path(), "outside/QA.md");
    std::fs::remove_dir_all(root.join("features/linked")).unwrap();
    std::os::unix::fs::symlink(
        directory.path().join("outside"),
        root.join("features/linked"),
    )
    .unwrap();
    // 未追跡で `.gitignore` の対象でない QA.md は読む
    write_qa_md(&root, "features/untracked/QA.md");
    std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    write_qa_md(&root, "ignored/QA.md");
    // 有効なリポジトリを指さない `.git` のファイルを置いたディレクトリ
    write_qa_md(&root, "placeholder/QA.md");
    std::fs::write(root.join("placeholder/.git"), "gitdir: /nonexistent\n").unwrap();
    write_qa_md(&root, "node_modules/package/QA.md");

    assert_eq!(
        dry_run_files(&sandbox, &root),
        ["QA.md", "features/untracked/QA.md"]
    );
}

#[test]
fn qa_import_outside_git_reads_every_qa_md_except_fixed_directory_names() {
    let sandbox = sandbox();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("notes");
    write_qa_md(&root, "QA.md");
    // git の管理下でなければ `.gitignore` は見ない
    std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    write_qa_md(&root, "ignored/QA.md");
    write_qa_md(&root, "node_modules/package/QA.md");
    write_qa_md(&root, "target/QA.md");

    assert_eq!(dry_run_files(&sandbox, &root), ["QA.md", "ignored/QA.md"]);
}

// macOS (APFS) は UTF-8 でないファイル名を作れないため linux に限る (CI は ubuntu)
#[cfg(target_os = "linux")]
#[test]
fn qa_import_in_a_git_work_tree_reads_qa_md_under_a_non_utf8_directory_name() {
    use std::os::unix::ffi::OsStrExt;

    let sandbox = sandbox();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("notes");
    let file = root
        .join(std::ffi::OsStr::from_bytes(b"feature-\xff"))
        .join("QA.md");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, "# QA\n\n- [ ] **一覧が開く**: 項目が並ぶ\n").unwrap();
    git(&root, &["init", "-q"]);

    let report = succeed_json(
        &sandbox,
        &[
            "qa",
            "import",
            "--dry-run",
            "--json",
            root.to_str().unwrap(),
        ],
    );
    assert_eq!(report["files"].as_array().unwrap().len(), 1, "{report}");
}

#[test]
fn qa_import_uses_the_repository_of_the_root_even_if_git_dir_is_inherited() {
    let sandbox = sandbox();
    let directory = tempfile::tempdir().unwrap();
    let other = directory.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q"]);
    // 別のリポジトリの GIT_DIR が使われると、その除外の設定でルートの QA.md が 1 件も出なくなる
    std::fs::create_dir_all(other.join(".git/info")).unwrap();
    std::fs::write(other.join(".git/info/exclude"), "*\n").unwrap();
    let root = directory.path().join("notes");
    write_qa_md(&root, "QA.md");
    git(&root, &["init", "-q"]);

    let output = command(
        &sandbox,
        &[
            "qa",
            "import",
            "--dry-run",
            "--json",
            root.to_str().unwrap(),
        ],
    )
    .env("GIT_DIR", other.join(".git"))
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["files"],
        serde_json::json!([root.canonicalize().unwrap().join("QA.md")])
    );
}

#[test]
fn qa_import_of_a_gitignored_root_reads_like_outside_git() {
    let sandbox = sandbox();
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repository");
    std::fs::create_dir_all(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    std::fs::write(repository.join(".gitignore"), "tmp/\n").unwrap();
    let root = repository.join("tmp/notes");
    write_qa_md(&root, "QA.md");
    write_qa_md(&root, "features/list/QA.md");

    assert_eq!(
        dry_run_files(&sandbox, &root),
        ["QA.md", "features/list/QA.md"]
    );
}

/// `directory` の下を `destination` へ再帰的に複製する。
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

/// `repository` で `git <args>` を成功させ、stdout の前後の空白を除いて返す。開発者・CI の git の設定
/// (署名・ユーザー名) に左右されないよう、commit に要る設定をここで渡す。
fn git(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args([
            "-c",
            "user.name=tansu-test",
            "-c",
            "user.email=tansu-test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// `repository` の作業ツリーをすべて commit し、その full SHA を返す。
fn commit_all(repository: &Path, message: &str) -> String {
    git(repository, &["add", "-A"]);
    git(repository, &["commit", "-q", "-m", message]);
    git(repository, &["rev-parse", "HEAD"])
}

/// `tansu <args>` を実行し、(exit code, stdout, stderr) を返す。
fn run(sandbox: &Sandbox, args: &[&str]) -> (Option<i32>, String, String) {
    let output = command(sandbox, args).output().unwrap();
    (
        output.status.code(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// `tansu qa verified --repo acme/notes --commit <commit> --json <options>` を実行し、(exit code, 出力の JSON) を返す。
fn verified(sandbox: &Sandbox, commit: &str, options: &[&str]) -> (Option<i32>, Value) {
    let mut args = vec![
        "qa",
        "verified",
        "--repo",
        "acme/notes",
        "--commit",
        commit,
        "--json",
    ];
    args.extend(options);
    let (code, stdout, stderr) = run(sandbox, &args);
    (
        code,
        serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{error}: {stdout} {stderr}")),
    )
}

/// `verified` の出力から、feature 名が `name` の要素。
fn feature_of<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["feature"] == name)
        .unwrap_or_else(|| panic!("feature `{name}` が無い: {report}"))
}

/// データベース `run` の行の数。
fn run_count(sandbox: &Sandbox) -> usize {
    query(sandbox, "run", &[]).len()
}

/// `usage.jsonl` の行を JSON にして返す (ファイルが無ければ空)。
fn usage_lines(sandbox: &Sandbox) -> Vec<Value> {
    std::fs::read_to_string(sandbox.tansu_home.path().join("usage.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// JSON の中のオブジェクトのキーを、入れ子 (配列の要素を含む) まで再帰的に `keys` に集める。
fn collect_keys(value: &Value, keys: &mut std::collections::BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                keys.insert(key.clone());
                collect_keys(value, keys);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_keys(item, keys)),
        _ => {}
    }
}

/// `fixtures/qa/sample-notes` を git の合成リポジトリ (origin は `acme/notes`) に複製して 1 つ目の commit を作り、
/// `qa import` で取り込む。(リポジトリを置いた一時ディレクトリ, ルート, 1 つ目の commit) を返す。
fn imported_repository(sandbox: &Sandbox) -> (TempDir, PathBuf, String) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("notes");
    copy_dir(
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/qa/sample-notes"
        )),
        &root,
    );
    // feature の実装に当たる、QA.md 以外のファイル
    std::fs::write(root.join("features/settings/theme.txt"), "dark\n").unwrap();
    git(&root, &["init", "-q"]);
    git(
        &root,
        &["remote", "add", "origin", "git@github.com:acme/notes.git"],
    );
    let first = commit_all(&root, "first");
    succeed(sandbox, &["qa", "import", "--json", root.to_str().unwrap()]);
    (directory, root, first)
}

#[test]
fn qa_record_adds_a_run_each_time_and_once_per_run_id() {
    let sandbox = sandbox();
    let (_directory, _root, first) = imported_repository(&sandbox);
    let before = run_count(&sandbox);
    let usage_before = usage_lines(&sandbox).len();

    let recorded = succeed_json(
        &sandbox,
        &[
            "qa",
            "record",
            "--repo",
            "acme/notes",
            "--feature",
            "note_list",
            "--item-name",
            "一覧の並び",
            "--result",
            "OK",
            "--commit",
            &first,
            "--note",
            "並びを確認した",
            "--image-url",
            "https://example.com/sample-notes/note_list/order-1.png",
            "--image-url",
            "https://example.com/sample-notes/note_list/order-(2).png",
            "--env",
            "iPhone 17 (iOS 27.0)",
            "--json",
        ],
    );
    let values = &recorded["values"];
    assert_eq!(values["result"]["value"], "ok");
    assert_eq!(values["source"]["value"], "record");
    assert_eq!(values["commit"]["value"], first);
    assert_eq!(values["note"]["value"], "並びを確認した");
    assert_eq!(values["env"]["value"], "iPhone 17 (iOS 27.0)");
    assert_eq!(
        values["image_url"]["value"],
        "https://example.com/sample-notes/note_list/order-1.png"
    );
    assert_eq!(
        recorded["body"],
        "![](<https://example.com/sample-notes/note_list/order-1.png>)\n![](<https://example.com/sample-notes/note_list/order-(2).png>)\n"
    );
    assert!(
        values["run_id"]["value"]
            .as_str()
            .unwrap()
            .starts_with("record:")
    );
    let recorded_at = values["recorded_at"]["value"].as_str().unwrap();
    assert!(
        is_date_time_with_offset(recorded_at),
        "recorded_at: {recorded_at}"
    );
    assert_eq!(run_count(&sandbox), before + 1);
    let item_id = values["item"]["value"][0].as_str().unwrap().to_owned();

    // 実行のたびに 1 行足す
    let text = succeed(
        &sandbox,
        &[
            "qa", "record", "--item", &item_id, "--result", "ng", "--commit", &first,
        ],
    );
    assert!(text.starts_with("run "), "{text}");
    assert_eq!(run_count(&sandbox), before + 2);

    // 同じ --run-id の 2 回目は行を増やさず、その行を置き換える
    for result in ["later", "ok"] {
        succeed(
            &sandbox,
            &[
                "qa", "record", "--item", &item_id, "--result", result, "--commit", &first,
                "--run-id", "retry-1",
            ],
        );
    }
    assert_eq!(run_count(&sandbox), before + 3);
    let retried = query(&sandbox, "run", &["--where", "run_id=retry-1"]);
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0]["values"]["result"]["value"], "ok");

    // --commit が無い・full SHA でない・項目が無い・結果が選択肢に無い・取り込みの run_id は書かない
    let (code, _, stderr) = run(
        &sandbox,
        &["qa", "record", "--item", &item_id, "--result", "ok"],
    );
    assert_ne!(code, Some(0));
    assert!(stderr.contains("--commit"), "{stderr}");
    for args in [
        vec![
            "--item",
            &item_id,
            "--result",
            "ok",
            "--commit",
            &first[..7],
        ],
        vec!["--item", "missing", "--result", "ok", "--commit", &first],
        vec![
            "--repo",
            "acme/notes",
            "--feature",
            "note_list",
            "--item-name",
            "無い項目",
            "--result",
            "ok",
            "--commit",
            &first,
        ],
        vec!["--item", &item_id, "--result", "done", "--commit", &first],
        vec![
            "--item",
            &item_id,
            "--result",
            "ok",
            "--commit",
            &first,
            "--image-url",
            "https://example.com/a.png\n## 見出し",
        ],
        vec![
            "--item", &item_id, "--result", "ok", "--commit", &first, "--run-id", "import:x",
        ],
    ] {
        let mut full = vec!["qa", "record"];
        full.extend(args);
        fail(&sandbox, &full);
    }
    assert_eq!(run_count(&sandbox), before + 3);

    // 実行の種類と日時だけを利用記録に 1 行ずつ足す (成功した 4 回・失敗した 6 回の qa record と、間の query)
    let usage = usage_lines(&sandbox);
    let records: Vec<&Value> = usage[usage_before..]
        .iter()
        .filter(|line| line["event"] == "qa_record")
        .collect();
    assert_eq!(records.len(), 4 + 6);
    assert_eq!(records[0].as_object().unwrap().len(), 2);
    assert!(is_date_time_with_offset(records[0]["at"].as_str().unwrap()));
    assert!(
        usage[usage_before..]
            .iter()
            .any(|line| line["event"] == "query")
    );
}

/// `text` が秒までの時差付きの日時 (`2026-10-12T09:00:00+09:00` か `2026-10-12T00:00:00Z`) の形か。
fn is_date_time_with_offset(text: &str) -> bool {
    let bytes = text.as_bytes();
    let offset = text.get("2026-10-12T09:00:00".len()..).unwrap_or_default();
    bytes.len() > 10
        && bytes[4] == b'-'
        && bytes[10] == b'T'
        && (offset == "Z" || (offset.len() == "+09:00".len() && offset.starts_with(['+', '-'])))
}

#[test]
fn qa_verified_returns_changed_features_and_unverified_items() {
    let sandbox = sandbox();
    let (_directory, root, first) = imported_repository(&sandbox);

    // 取り込んだ run の commit は合成の SHA でリポジトリに無く、run の無い item もあるので再テストが要る
    let (code, report) = verified(&sandbox, &first, &[]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["ok"], false);
    assert_eq!(report["repo"], "acme/notes");
    assert_eq!(report["commit"], first);
    let note_list = feature_of(&report, "note_list");
    // last_verified_commit (合成の SHA) がリポジトリに無いので、ディレクトリの全ファイル (QA.md を除く) を返す。
    // fixture のディレクトリには QA.md しか無いので空
    assert_eq!(
        note_list["last_verified_commit"],
        "3f2a9c1e5b7d4a6f8c0e2b4d6f8a1c3e5b7d9f0a"
    );
    assert_eq!(note_list["changed_files"], json!([]));
    let unverified: Vec<&str> = note_list["unverified_items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(unverified, ["空の案内", "ピン留めの固定", "スワイプで削除"]);
    // frontmatter の last_verified_commit が null の feature は、確かめた commit が無いのでディレクトリの全ファイルを返す
    let settings = feature_of(&report, "settings");
    assert_eq!(settings["last_verified_commit"], Value::Null);
    assert_eq!(
        settings["changed_files"],
        json!(["features/settings/theme.txt"])
    );
    // ルート直下の feature は diff しない
    assert_eq!(feature_of(&report, "_root")["changed_files"], json!([]));

    // すべての item を 1 つ目の commit で ok にすると、再テストは要らない
    for item in query(&sandbox, "item", &[]) {
        succeed(
            &sandbox,
            &[
                "qa",
                "record",
                "--item",
                item["id"].as_str().unwrap(),
                "--result",
                "ok",
                "--commit",
                &first,
            ],
        );
    }
    let (code, report) = verified(&sandbox, &first, &[]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["ok"], true);
    assert_eq!(report["features"].as_array().unwrap().len(), 4);
    assert_eq!(
        feature_of(&report, "note_list")["last_verified_commit"],
        first
    );

    // note_list のディレクトリを変えた commit では、その feature が changed_files 付きで返る
    std::fs::write(root.join("features/note_list/list.txt"), "並び替え\n").unwrap();
    // QA.md の変更は feature の実装の変更に数えない
    let qa_md = root.join("features/note_editor/QA.md");
    let mut text = std::fs::read_to_string(&qa_md).unwrap();
    text.push_str("\n<!-- 追記 -->\n");
    std::fs::write(&qa_md, text).unwrap();
    let second = commit_all(&root, "second");
    let (code, report) = verified(&sandbox, &second, &[]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(report["ok"], false);
    assert_eq!(
        feature_of(&report, "note_list")["changed_files"],
        json!(["features/note_list/list.txt"])
    );
    for name in ["_root", "note_editor", "settings"] {
        assert_eq!(
            feature_of(&report, name)["changed_files"],
            json!([]),
            "{name}"
        );
    }
    // --diff-base を渡すと、そこからディレクトリが変わった feature だけを判定する
    let (code, scoped) = verified(&sandbox, &second, &["--diff-base", &first]);
    assert_eq!(code, Some(1), "{scoped}");
    assert_eq!(scoped["features"].as_array().unwrap().len(), 1);
    assert_eq!(scoped["features"][0]["feature"], "note_list");

    // その feature の項目を新しい commit で記録すると、再テストは要らない (同じ秒の記録でも、後に足した run を最後とみなす)
    succeed(
        &sandbox,
        &[
            "qa",
            "record",
            "--repo",
            "acme/notes",
            "--feature",
            "note_list",
            "--item-name",
            "検索で絞り込める",
            "--result",
            "skip",
            "--commit",
            &second,
        ],
    );
    let (code, report) = verified(&sandbox, &second, &[]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(
        feature_of(&report, "note_list")["last_verified_commit"],
        second
    );

    // run の無い item があると unverified_items に入り、exit 1
    let note_list_id = query(&sandbox, "feature", &["--where", "name=note_list"])[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    upsert(
        &sandbox,
        "item",
        json!({"id": "new-item", "values": {"feature": [note_list_id], "name": "新しい項目"}}),
    );
    let (code, report) = verified(&sandbox, &second, &[]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(
        feature_of(&report, "note_list")["unverified_items"],
        json!([{"id": "new-item", "name": "新しい項目"}])
    );
    assert_eq!(feature_of(&report, "note_list")["changed_files"], json!([]));

    // エラーは exit 2 (無い project・full SHA でない commit・リポジトリに無い commit・--json 無し)
    for args in [
        vec!["--repo", "acme/missing", "--commit", &second, "--json"],
        vec!["--repo", "acme/notes", "--commit", &second[..7], "--json"],
        vec![
            "--repo",
            "acme/notes",
            "--commit",
            &"0".repeat(40),
            "--json",
        ],
        vec!["--repo", "acme/notes", "--commit", &second],
    ] {
        let mut full = vec!["qa", "verified"];
        full.extend(args.iter().copied());
        let (code, stdout, stderr) = run(&sandbox, &full);
        assert_eq!(code, Some(2), "{args:?}: {stderr}");
        assert!(stdout.is_empty());
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
}

#[test]
fn qa_verified_sees_japanese_paths_and_both_sides_of_a_rename() {
    let sandbox = sandbox();
    let (_directory, root, first) = imported_repository(&sandbox);
    for item in query(&sandbox, "item", &[]) {
        succeed(
            &sandbox,
            &[
                "qa",
                "record",
                "--item",
                item["id"].as_str().unwrap(),
                "--result",
                "ok",
                "--commit",
                &first,
            ],
        );
    }

    // 日本語のパスも、git がエスケープした形ではなくそのままのパスで返す
    std::fs::write(root.join("features/note_list/画面.txt"), "一覧\n").unwrap();
    let second = commit_all(&root, "second");
    let (code, scoped) = verified(&sandbox, &second, &["--diff-base", &first]);
    assert_eq!(code, Some(1), "{scoped}");
    assert_eq!(scoped["features"].as_array().unwrap().len(), 1, "{scoped}");
    assert_eq!(
        feature_of(&scoped, "note_list")["changed_files"],
        json!(["features/note_list/画面.txt"])
    );

    // ほかの feature のディレクトリへ移したファイルは、移動元と移動先の両方の feature の変更に数える
    git(
        &root,
        &[
            "mv",
            "features/settings/theme.txt",
            "features/note_editor/theme.txt",
        ],
    );
    let third = commit_all(&root, "third");
    let (code, scoped) = verified(&sandbox, &third, &["--diff-base", &second]);
    assert_eq!(code, Some(1), "{scoped}");
    let names: Vec<&str> = scoped["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature["feature"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["note_editor", "settings"], "{scoped}");
    assert_eq!(
        feature_of(&scoped, "settings")["changed_files"],
        json!(["features/settings/theme.txt"])
    );
    assert_eq!(
        feature_of(&scoped, "note_editor")["changed_files"],
        json!(["features/note_editor/theme.txt"])
    );
}

#[test]
fn qa_export_pr_summary_shows_the_latest_run_of_each_item() {
    let sandbox = sandbox();
    let (_directory, root, first) = imported_repository(&sandbox);
    let recorded = succeed_json(
        &sandbox,
        &[
            "qa",
            "record",
            "--repo",
            "acme/notes",
            "--feature",
            "note_list",
            "--item-name",
            "一覧の並び",
            "--result",
            "ok",
            "--commit",
            &first,
            "--image-url",
            "https://example.com/sample-notes/note_list/order-new.png",
            "--json",
        ],
    );
    let recorded_date = &recorded["values"]["recorded_at"]["value"].as_str().unwrap()[..10];
    std::fs::write(root.join("features/note_list/list.txt"), "並び替え\n").unwrap();
    let second = commit_all(&root, "second");
    succeed(
        &sandbox,
        &[
            "qa",
            "record",
            "--repo",
            "acme/notes",
            "--feature",
            "note_list",
            "--item-name",
            "検索で絞り込める",
            "--result",
            "ng",
            "--commit",
            &second,
        ],
    );

    let summary = succeed(
        &sandbox,
        &[
            "qa",
            "export",
            "--repo",
            "acme/notes",
            "--format",
            "pr-summary",
        ],
    );
    assert!(
        summary.starts_with("## QA の記録 (acme/notes)\n"),
        "{summary}"
    );
    // 見出しの feature 名は markdown の記号 (`_` 等) をエスケープして出す
    assert!(summary.contains("### note\\_list\n"), "{summary}");
    assert!(
        summary.contains("| 項目 | 結果 | 確認日 | 画像 | commit |"),
        "{summary}"
    );
    assert!(
        summary.contains(&format!(
            "| 一覧の並び | OK | {recorded_date} | [画像](<https://example.com/sample-notes/note_list/order-new.png>) | `{}` |",
            &first[..7]
        )),
        "{summary}"
    );
    assert!(
        summary.contains(&format!(
            "| 検索で絞り込める | NG | {recorded_date} | — | `{}` |",
            &second[..7]
        )),
        "{summary}"
    );
    // 取り込んだ run も最後の run として出る (確認日は QA.md の確認日、commit は frontmatter の合成の SHA)
    assert!(
        summary.contains("| 画像を貼れる | NG | 2026-10-07 |"),
        "{summary}"
    );
    // run の無い item は出さない
    assert!(!summary.contains("空の案内"), "{summary}");

    // --since はその commit の子孫で記録した run だけを出す (その commit 自身の run は出さない)
    let since = succeed(
        &sandbox,
        &[
            "qa",
            "export",
            "--repo",
            "acme/notes",
            "--format",
            "pr-summary",
            "--since",
            &first,
        ],
    );
    assert!(since.contains("検索で絞り込める"), "{since}");
    assert!(!since.contains("一覧の並び"), "{since}");
    assert!(since.contains("### note\\_list\n"), "{since}");
    assert!(!since.contains("### note\\_editor"), "{since}");

    fail(
        &sandbox,
        &[
            "qa",
            "export",
            "--repo",
            "acme/missing",
            "--format",
            "pr-summary",
        ],
    );
}

#[test]
fn cli_contract_lists_every_json_key_the_qa_commands_print() {
    let sandbox = sandbox();
    let (_directory, root, first) = imported_repository(&sandbox);
    let mut keys = std::collections::BTreeSet::new();
    collect_keys(
        &succeed_json(
            &sandbox,
            &["qa", "import", "--json", root.to_str().unwrap()],
        ),
        &mut keys,
    );
    let recorded = succeed_json(
        &sandbox,
        &[
            "qa",
            "record",
            "--repo",
            "acme/notes",
            "--feature",
            "note_list",
            "--item-name",
            "一覧の並び",
            "--result",
            "ok",
            "--commit",
            &first,
            "--note",
            "メモ",
            "--image-url",
            "https://example.com/a.png",
            "--env",
            "env",
            "--json",
        ],
    );
    collect_keys(&recorded, &mut keys);
    collect_keys(&verified(&sandbox, &first, &[]).1, &mut keys);

    let contract = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../documents/cli-contract.md"
    ))
    .unwrap();
    let missing: Vec<&String> = keys
        .iter()
        .filter(|key| !contract.contains(&format!("`{key}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "documents/cli-contract.md に無いキー: {missing:?}"
    );
}

#[test]
fn closed_stdout_is_not_an_error() {
    let sandbox = sandbox();
    succeed(&sandbox, &["db", "create", "tasks"]);
    // 読み手が先に閉じたパイプ (`tansu db list | head -c 0` に当たる) に書かせる
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let output = command(&sandbox, &["db", "list"])
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status: {:?}, stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
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
fn libsql_url_without_an_auth_token_fails() {
    let sandbox = sandbox();
    let stderr = assert_failed(
        command(&sandbox, &["db", "list"])
            .env("TANSU_DB_URL", "libsql://example.turso.io")
            .output()
            .unwrap(),
    );
    assert!(stderr.contains("TANSU_DB_AUTH_TOKEN"), "{stderr}");
}

#[test]
fn unreachable_libsql_url_fails_without_leaking_the_token() {
    let sandbox = sandbox();
    // 127.0.0.1 のポート 1 は待ち受けが無く、外部のネットワークに出ずに接続が拒否される
    let stderr = assert_failed(
        command(&sandbox, &["db", "list"])
            .env("TANSU_DB_URL", "libsql://127.0.0.1:1")
            .env("TANSU_DB_AUTH_TOKEN", "secret-token-value")
            .output()
            .unwrap(),
    );
    assert!(!stderr.contains("secret-token-value"), "{stderr}");
}

/// `tansu mcp` に接続した MCP のクライアント。
type McpClient = RunningService<RoleClient, ()>;

/// `sandbox` を `TANSU_HOME` にして `tansu mcp` を子プロセスとして起動し、MCP のクライアントとして接続する。
async fn mcp_client(sandbox: &Sandbox) -> McpClient {
    ().serve(
        TokioChildProcess::new(tokio::process::Command::from(command(sandbox, &["mcp"]))).unwrap(),
    )
    .await
    .unwrap()
}

/// MCP の tool `name` を `arguments` (JSON の object) で呼び、(isError か, 本文のテキスト) を返す。本文が無ければ空文字。
async fn call_mcp(client: &McpClient, name: &'static str, arguments: Value) -> (bool, String) {
    let result = client
        .call_tool(
            CallToolRequestParams::new(name).with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    (
        result.is_error == Some(true),
        result
            .content
            .iter()
            .map(|content| content.as_text().unwrap().text.as_str())
            .collect(),
    )
}

/// MCP の tool `name` が成功したことを確かめ、本文を JSON として返す。
async fn call_mcp_json(client: &McpClient, name: &'static str, arguments: Value) -> Value {
    let (is_error, text) = call_mcp(client, name, arguments).await;
    assert!(!is_error, "{name} が失敗した: {text}");
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{error}: {text}"))
}

/// `usage.jsonl` の `event` を並びのまま返す。
fn usage_events(sandbox: &Sandbox) -> Vec<String> {
    usage_lines(sandbox)
        .iter()
        .map(|line| line["event"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn mcp_lists_one_tool_per_cli_subcommand() {
    let sandbox = sandbox();
    let client = mcp_client(&sandbox).await;
    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "db_create",
            "db_list",
            "db_schema",
            "delete",
            "export",
            "qa_export",
            "qa_import",
            "qa_record",
            "qa_verified",
            "query",
            "upsert",
        ]
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_writes_and_reads_rows_in_the_same_shape_as_the_cli() {
    let sandbox = sandbox();
    let client = mcp_client(&sandbox).await;

    let created = call_mcp_json(&client, "db_create", json!({"name": "tasks"})).await;
    assert_eq!(created, succeed_json(&sandbox, &["db", "schema", "tasks"]));
    let changed = call_mcp_json(
        &client,
        "db_schema",
        json!({"name": "tasks", "add": ["title:text", "status:select:todo,done"], "unique": ["title"]}),
    )
    .await;
    assert_eq!(changed, succeed_json(&sandbox, &["db", "schema", "tasks"]));
    assert_eq!(
        call_mcp_json(&client, "db_list", json!({})).await,
        succeed_json(&sandbox, &["db", "list"])
    );

    let written = call_mcp_json(
        &client,
        "upsert",
        json!({"database": "tasks", "json": {"id": "t1", "body": "本文", "values": {"title": "a", "status": "todo"}}}),
    )
    .await;
    // 行の JSON を文字列で渡してもよい
    call_mcp_json(
        &client,
        "upsert",
        json!({"database": "tasks", "json": json!({"id": "t2", "values": {"title": "b", "status": "done"}}).to_string()}),
    )
    .await;
    assert_eq!(written, query(&sandbox, "tasks", &[])[0]);

    let queried = call_mcp_json(
        &client,
        "query",
        json!({"database": "tasks", "where": ["status=done"], "sort": "title:desc", "limit": 5}),
    )
    .await;
    assert_eq!(
        queried,
        succeed_json(
            &sandbox,
            &[
                "query",
                "tasks",
                "--json",
                "--where",
                "status=done",
                "--sort",
                "title:desc",
                "--limit",
                "5"
            ]
        )
    );
    assert_eq!(row_ids(queried.as_array().unwrap()), ["t2"]);
    assert_eq!(
        call_mcp_json(
            &client,
            "export",
            json!({"database": "tasks", "format": "json"})
        )
        .await,
        succeed_json(&sandbox, &["export", "tasks", "--format", "json"])
    );
    let (is_error, markdown) = call_mcp(
        &client,
        "export",
        json!({"database": "tasks", "format": "markdown"}),
    )
    .await;
    assert!(!is_error, "{markdown}");
    // CLI は結果の後ろに改行を 1 つ足して出力する
    assert_eq!(
        Some(markdown.as_str()),
        succeed(&sandbox, &["export", "tasks", "--format", "markdown"]).strip_suffix('\n')
    );

    assert_eq!(
        call_mcp(
            &client,
            "delete",
            json!({"database": "tasks", "row_id": "t1"})
        )
        .await,
        (false, String::new())
    );
    assert_eq!(row_ids(&query(&sandbox, "tasks", &[])), ["t2"]);

    // 失敗は CLI の stderr と同じ理由を本文にした isError の結果になる
    let (is_error, message) = call_mcp(&client, "query", json!({"database": "missing"})).await;
    assert!(is_error);
    assert_eq!(
        format!("tansu: {message}\n"),
        fail(&sandbox, &["query", "missing", "--json"])
    );
    client.cancel().await.unwrap();

    let events = usage_events(&sandbox);
    assert_eq!(events[0], "mcp");
    let mcp_events: Vec<&str> = events
        .iter()
        .filter_map(|event| event.strip_prefix("mcp_"))
        .collect();
    assert_eq!(
        mcp_events,
        [
            "db_create",
            "db_schema",
            "db_list",
            "upsert",
            "upsert",
            "query",
            "export",
            "export",
            "delete",
            "query"
        ]
    );
}

#[tokio::test]
async fn mcp_records_a_run_and_reports_verified() {
    let sandbox = sandbox();
    let (_directory, root, first) = imported_repository(&sandbox);
    let client = mcp_client(&sandbox).await;

    let dry = call_mcp_json(
        &client,
        "qa_import",
        json!({"paths": [root.to_str().unwrap()], "dry_run": true}),
    )
    .await;
    assert_eq!(dry["dry_run"], true);
    assert_eq!(
        dry,
        succeed_json(
            &sandbox,
            &[
                "qa",
                "import",
                "--dry-run",
                "--json",
                root.to_str().unwrap()
            ]
        )
    );

    // 再テストが要る時も isError にはせず、ok が false の結果を返す
    let verified_before = call_mcp_json(
        &client,
        "qa_verified",
        json!({"repo": "acme/notes", "commit": first}),
    )
    .await;
    assert_eq!(verified_before["ok"], false);
    assert_eq!(verified_before, verified(&sandbox, &first, &[]).1);

    let before = run_count(&sandbox);
    let recorded = call_mcp_json(
        &client,
        "qa_record",
        json!({
            "repo": "acme/notes",
            "feature": "note_list",
            "item_name": "空の案内",
            "result": "ok",
            "commit": first,
            "image_url": ["https://example.com/sample-notes/note_list/empty.png"],
            "run_id": "mcp-test-1",
        }),
    )
    .await;
    assert_eq!(recorded["values"]["result"]["value"], "ok");
    assert_eq!(recorded["values"]["source"]["value"], "record");
    assert_eq!(recorded["values"]["run_id"]["value"], "mcp-test-1");
    assert_eq!(run_count(&sandbox), before + 1);
    assert!(query(&sandbox, "run", &["--where", "run_id=mcp-test-1"]).contains(&recorded));

    let verified_after = call_mcp_json(
        &client,
        "qa_verified",
        json!({"repo": "acme/notes", "commit": first}),
    )
    .await;
    assert_eq!(verified_after, verified(&sandbox, &first, &[]).1);
    let note_list = feature_of(&verified_after, "note_list");
    assert_eq!(note_list["last_verified_commit"], first);
    let unverified: Vec<&str> = note_list["unverified_items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(unverified, ["ピン留めの固定", "スワイプで削除"]);

    let (is_error, summary) = call_mcp(
        &client,
        "qa_export",
        json!({"repo": "acme/notes", "format": "pr-summary"}),
    )
    .await;
    assert!(!is_error, "{summary}");
    assert_eq!(
        summary,
        succeed(
            &sandbox,
            &[
                "qa",
                "export",
                "--repo",
                "acme/notes",
                "--format",
                "pr-summary"
            ]
        )
        .trim_end()
    );

    // 項目の指定は item か repo / feature / item_name の組のどちらか一方
    let (is_error, message) = call_mcp(
        &client,
        "qa_record",
        json!({"item": "x", "repo": "acme/notes", "result": "ok", "commit": first}),
    )
    .await;
    assert!(is_error, "{message}");
    assert_eq!(run_count(&sandbox), before + 1);
    client.cancel().await.unwrap();
}
