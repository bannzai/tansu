//! `tansu serve` の router に 1 リクエストずつ渡し、`Host` の検査・書き込みの CSRF 対策・利用記録・画面が使う API を確かめる。
//! `TANSU_HOME` はテストごとの一時ディレクトリにする (`.claude/rules/sqlite-database.md`)。

use std::path::Path;
use std::process::Command;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tansu_core::config::{DbLocation, Settings};
use tansu_core::db;
use tansu_core::qa::import;
use tansu_server::{DEFAULT_HOST, DEFAULT_PORT, ServeOptions, router};
use tempfile::TempDir;
use tower::ServiceExt;

/// 既定の `--host` と `--port` で待ち受けた時に、ブラウザが送る `Host`。
const HOST: &str = "127.0.0.1:7979";

/// `home` を `TANSU_HOME` にし、その下の DB を開く設定。
fn settings(home: &TempDir) -> Settings {
    Settings {
        home: home.path().to_owned(),
        db: DbLocation::Local(home.path().join("tansu.sqlite")),
    }
}

/// 既定の `--host` / `--port` に `--allow-host <allow_hosts>` を足した router。
fn app(home: &TempDir, allow_hosts: &[&str]) -> Router {
    router(
        settings(home),
        &ServeOptions {
            host: DEFAULT_HOST.into(),
            port: DEFAULT_PORT,
            allow_hosts: allow_hosts.iter().map(|host| (*host).to_owned()).collect(),
        },
        &[],
    )
}

/// router に `request` を渡し、(状態コード, 本文の JSON。本文が空なら null) を返す。
async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
}

/// `Host: <host>` の GET。
fn get(path: &str, host: &str) -> Request<Body> {
    Request::get(path)
        .header(header::HOST, host)
        .body(Body::empty())
        .unwrap()
}

/// 画面と同じ形 (`Host: 127.0.0.1:7979`、同じオリジンの `Origin`、JSON の本文) の POST。
fn post_json(path: &str, body: &Value) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, format!("http://{HOST}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// `headers` を付けた `POST /api/usage/web-open` (`Host` は既定の待ち受けのもの)。
fn web_open(headers: &[(&str, &str)]) -> Request<Body> {
    headers
        .iter()
        .fold(
            Request::post("/api/usage/web-open").header(header::HOST, HOST),
            |request, (name, value)| request.header(*name, *value),
        )
        .body(Body::from("{}"))
        .unwrap()
}

/// `usage.jsonl` の行を JSON にして返す (ファイルが無ければ空)。
fn usage_lines(home: &TempDir) -> Vec<Value> {
    std::fs::read_to_string(home.path().join("usage.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn host_outside_the_allow_list_is_rejected() {
    let home = tempfile::tempdir().unwrap();
    let app = app(&home, &[]);
    for host in ["127.0.0.1:7979", "localhost:7979", "LOCALHOST:7979"] {
        assert_eq!(
            send(&app, get("/api/databases", host)).await.0,
            StatusCode::OK,
            "{host}"
        );
    }
    for host in [
        "evil.example:7979",
        "evil.example",
        "127.0.0.1:8080",
        "127.0.0.1",
        "127.0.0.1.evil.example:7979",
    ] {
        let (status, body) = send(&app, get("/api/databases", host)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host}");
        assert!(body["error"].as_str().unwrap().contains("Host"), "{body}");
    }
    let without_host = Request::get("/api/databases").body(Body::empty()).unwrap();
    assert_eq!(send(&app, without_host).await.0, StatusCode::FORBIDDEN);
    // 画面のファイルも同じ検査を通る
    assert_eq!(
        send(&app, get("/", "evil.example:7979")).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn allow_host_adds_a_host_name() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(
        send(
            &app(&home, &[]),
            get("/api/databases", "mac.tailnet.ts.net:7979")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &app(&home, &["mac.tailnet.ts.net"]),
            get("/api/databases", "Mac.Tailnet.ts.net:7979")
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn writes_from_another_site_or_not_in_json_are_rejected() {
    let home = tempfile::tempdir().unwrap();
    let app = app(&home, &[]);
    let json = ("content-type", "application/json");
    let same_origin = ("origin", "http://127.0.0.1:7979");
    let cases: [(&[(&str, &str)], StatusCode); 11] = [
        (&[json, same_origin], StatusCode::NO_CONTENT),
        // curl など Origin を送らない呼び出し
        (&[json], StatusCode::NO_CONTENT),
        (
            &[("content-type", "application/json; charset=utf-8")],
            StatusCode::NO_CONTENT,
        ),
        (
            &[json, same_origin, ("sec-fetch-site", "same-origin")],
            StatusCode::NO_CONTENT,
        ),
        (
            &[json, ("origin", "http://evil.example")],
            StatusCode::FORBIDDEN,
        ),
        (&[json, ("origin", "null")], StatusCode::FORBIDDEN),
        (
            &[json, same_origin, ("sec-fetch-site", "cross-site")],
            StatusCode::FORBIDDEN,
        ),
        // 別のサイトの form やプリフライトなしの fetch が送れる形式
        (
            &[("content-type", "text/plain"), same_origin],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            &[("content-type", "application/x-www-form-urlencoded")],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            &[("content-type", "multipart/form-data; boundary=x")],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (&[], StatusCode::UNSUPPORTED_MEDIA_TYPE),
    ];
    for (headers, expected) in cases {
        assert_eq!(
            send(&app, web_open(headers)).await.0,
            expected,
            "{headers:?}"
        );
    }
    // 受け付けた 4 つだけが利用記録に書かれている
    assert_eq!(usage_lines(&home).len(), 4);
}

#[tokio::test]
async fn web_open_writes_one_usage_line() {
    let home = tempfile::tempdir().unwrap();
    let (status, _) = send(
        &app(&home, &[]),
        post_json("/api/usage/web-open", &json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let lines = usage_lines(&home);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["event"], "web_open");
    assert!(lines[0]["at"].is_string());
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

/// `fixtures/qa/sample-notes` を一時ディレクトリの git リポジトリ `notes` (origin 無し。project の `repo` は
/// ディレクトリ名の `notes`) に複製して commit し、QA 層に取り込む。(リポジトリを置いた一時ディレクトリ, HEAD) を返す。
async fn imported_repository(home: &TempDir) -> (TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("notes");
    copy_dir(
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/qa/sample-notes"
        )),
        &root,
    );
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    import::import(&db::open(&settings(home)).await.unwrap(), &[root])
        .await
        .unwrap();
    (directory, head)
}

/// 行の JSON の、プロパティ `property` の値。
fn value_of<'a>(row: &'a Value, property: &str) -> &'a Value {
    &row["values"][property]["value"]
}

#[tokio::test]
async fn qa_session_lists_items_and_record_replaces_the_latest_run() {
    let home = tempfile::tempdir().unwrap();
    let app = app(&home, &[]);
    let (_directory, head) = imported_repository(&home).await;
    let session_path = "/api/qa/session?repo=notes&feature=note_list";

    let (status, session) = send(&app, get(session_path, HOST)).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["head_commit"], head);
    let items = session["items"].as_array().unwrap();
    let names: Vec<&str> = items
        .iter()
        .map(|item| value_of(&item["item"], "name").as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "一覧の並び",
            "検索で絞り込める",
            "空の案内",
            "ピン留めの固定",
            "スワイプで削除"
        ]
    );
    // チェック済みの項目は取り込んだ ok の run を持ち、未チェックの項目は run を持たない
    assert_eq!(value_of(&items[0]["latest_run"], "result"), "ok");
    assert_eq!(items[2]["latest_run"], Value::Null);

    let item_id = items[2]["item"]["id"].as_str().unwrap().to_owned();
    // 同じ run_id での選び直しは、run を足さずに置き換える
    for result in ["ng", "later"] {
        let (status, run) = send(
            &app,
            post_json(
                "/api/qa/record",
                &json!({
                    "item_id": item_id,
                    "result": result,
                    "commit": head,
                    "note": "案内の文言が違う",
                    "env": "Chromium",
                    "run_id": "web:session:empty",
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{run}");
        assert_eq!(value_of(&run, "result"), result);
    }
    let (_, session) = send(&app, get(session_path, HOST)).await;
    let latest_run = &session["items"][2]["latest_run"];
    assert_eq!(value_of(latest_run, "result"), "later");
    assert_eq!(value_of(latest_run, "commit"), head.as_str());
    assert_eq!(value_of(latest_run, "source"), "record");
    assert_eq!(value_of(latest_run, "note"), "案内の文言が違う");
    assert_eq!(value_of(latest_run, "env"), "Chromium");
    let (_, runs) = send(&app, get("/api/databases/run/rows", HOST)).await;
    assert_eq!(
        runs["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|run| value_of(run, "run_id") == "web:session:empty")
            .count(),
        1
    );
}

#[tokio::test]
async fn qa_errors_are_returned_as_bad_request() {
    let home = tempfile::tempdir().unwrap();
    let app = app(&home, &[]);
    let (_directory, _head) = imported_repository(&home).await;
    let (status, body) = send(
        &app,
        get("/api/qa/session?repo=notes&feature=no_such_feature", HOST),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("no_such_feature"),
        "{body}"
    );
    let (_, session) = send(
        &app,
        get("/api/qa/session?repo=notes&feature=note_list", HOST),
    )
    .await;
    let (status, body) = send(
        &app,
        post_json(
            "/api/qa/record",
            &json!({"item_id": session["items"][0]["item"]["id"], "result": "ok", "commit": "HEAD"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn rows_can_be_listed_and_edited_with_new_options() {
    let home = tempfile::tempdir().unwrap();
    let app = app(&home, &[]);
    let (_directory, _head) = imported_repository(&home).await;

    let (status, database) = send(
        &app,
        post_json(
            "/api/databases/item/schema",
            &json!({"add": ["labels:multi_select:課金"]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{database}");
    let labels = database["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|property| property["name"] == "labels")
        .unwrap();
    assert_eq!(labels["kind"]["options"][0]["name"], "課金");

    let (_, items) = send(&app, get("/api/databases/item/rows", HOST)).await;
    assert_eq!(items["database"]["name"], "item");
    let item_id = items["rows"][0]["id"].as_str().unwrap().to_owned();
    let (status, row) = send(
        &app,
        post_json(
            "/api/databases/item/rows",
            &json!({"id": item_id, "body": "手順", "values": {"labels": ["課金"]}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    let (_, items) = send(&app, get("/api/databases/item/rows", HOST)).await;
    let edited = items["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == item_id.as_str())
        .unwrap();
    assert_eq!(value_of(edited, "labels"), &json!(["課金"]));
    assert_eq!(edited["body"], "手順");

    // 選択肢に無い値・無いデータベースは書かない
    let (status, _) = send(
        &app,
        post_json(
            "/api/databases/item/rows",
            &json!({"id": item_id, "values": {"labels": ["無い"]}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(&app, get("/api/databases/no_such_database/rows", HOST)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
