//! `qa import` が `fixtures/qa/` の合成 QA.md を 4 つの表に取り込み、QA.md の行を落とさず、2 回目で行が増えず、
//! `--dry-run` で何も書かないことを確かめる。`TANSU_HOME` はテストごとの一時ディレクトリ。

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tansu_core::config::{EnvVars, Settings};
use tansu_core::db::{self, rows, schema};
use tansu_core::model::Row;
use tansu_core::named_json;
use tansu_core::qa::import::{ImportCounts, ImportReport, dry_run, feature_body, import};
use tansu_core::qa::qa_md;

/// `fixtures/qa/sample-notes` の絶対パス。
fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/qa/sample-notes")
}

/// `fixtures/qa/lossless-notes` (再確認・注記付きの確認日・同じ名前の項目・セクション・地の文を含む合成 QA.md) の絶対パス。
fn lossless_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/qa/lossless-notes")
        .canonicalize()
        .unwrap()
}

/// `report` が読んだすべてのファイルの行数の合計。
fn total_lines(report: &ImportReport) -> usize {
    report
        .files
        .iter()
        .map(|file| std::fs::read_to_string(file).unwrap().lines().count())
        .sum()
}

/// どこにも入らなかった行が無く、すべての行が構造として読んだ行か本文に残した行のどちらか 1 つに数えられていることを確かめる。
fn assert_every_line_is_accounted(report: &ImportReport) {
    assert!(report.unaccounted.is_empty(), "{:?}", report.unaccounted);
    assert_eq!(report.lines.body, report.skipped.len());
    assert_eq!(
        report.lines.structured + report.lines.body,
        total_lines(report)
    );
}

/// JSON の中の文字列を、入れ子 (配列・object の値) まで `text` に 1 行ずつ足す。
fn collect_strings(value: &Value, text: &mut String) {
    match value {
        Value::String(string) => {
            text.push_str(string);
            text.push('\n');
        }
        Value::Array(values) => values.iter().for_each(|value| collect_strings(value, text)),
        Value::Object(object) => object
            .values()
            .for_each(|value| collect_strings(value, text)),
        _ => {}
    }
}

/// 取り込んだ 4 つの表の、すべての値とすべての本文をつないだ文字列。feature の本文は `## QA.md の原文` より前だけを使う
/// (原文を丸ごと持つので、そこまで含めると落ちた行も見つかってしまうため)。
async fn imported_text(connection: &libsql::Connection) -> String {
    let mut text = String::new();
    for database in ["project", "feature", "item", "run"] {
        for row in query_named(connection, database, &[]).await {
            text.push_str(
                row["body"]
                    .as_str()
                    .unwrap()
                    .split("## QA.md の原文")
                    .next()
                    .unwrap(),
            );
            text.push('\n');
            for value in row["values"].as_object().unwrap().values() {
                collect_strings(&value["value"], &mut text);
            }
        }
    }
    text
}

/// DB に値として入らない、QA.md の書式の語 (frontmatter のキー・記録と自動化の見出し・img / details / summary のタグと属性・
/// setup-qa の書式が決めている summary の文字列・null)。
const FORMAT_WORDS: &[&str] = &[
    "feature",
    "verification",
    "last_verified_commit",
    "last_verified_at",
    "null",
    "自動化",
    "確認日",
    "再確認日",
    "スキップ",
    "失敗",
    "img",
    "src",
    "width",
    "320",
    "details",
    "summary",
    "/details",
    "/summary",
    "動作確認スクショ",
    "動作確認エビデンス",
];

/// `report` が読んだ QA.md の行のうち、語 (2 文字以上の、英数字・かな漢字と `.` `/` `-` `_` の並び。[`FORMAT_WORDS`] を除く)
/// のどれかが `imported` に無い行 (`ファイル:行番号: 内容`)。空行と、`---`・`（未実行）`・`#### 動作確認` の書式だけの行は見ない。QA.md の各行を DB と照合した Issue #29 の実測と同じ見方。
fn lines_missing_from(report: &ImportReport, imported: &str) -> Vec<String> {
    let mut missing = Vec::new();
    for file in &report.files {
        let text = std::fs::read_to_string(file).unwrap();
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed == "---"
                || trimmed == "（未実行）"
                || trimmed == "#### 動作確認"
            {
                continue;
            }
            let has_missing_word = trimmed
                .split(|c: char| !(c.is_alphanumeric() || "./-_".contains(c)))
                .filter(|word| word.chars().count() >= 2 && !FORMAT_WORDS.contains(word))
                .any(|word| !imported.contains(word));
            if has_missing_word {
                missing.push(format!("{file}:{}: {line}", index + 1));
            }
        }
    }
    missing
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

    let report = import(&connection, std::slice::from_ref(&root))
        .await
        .unwrap();

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
    assert_eq!(report.skipped.len(), 38);
    assert_every_line_is_accounted(&report);
    assert_eq!(
        lines_missing_from(&report, &imported_text(&connection).await),
        Vec::<String>::new()
    );

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
        json!({
            "project": [project_id],
            "name": "note_editor",
            "verification": "agent-browser, manual",
            "path": "features/note_editor",
            "last_verified_commit": "a1b2c3d4e5f60718293a4b5c6d7e8f90ab12cd34",
            "last_verified_at": "2026-10-07",
        })
    );
    assert_eq!(features[0]["values"]["path"]["value"], "");
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
            "section": "1. 編集",
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
            "run_id": format!("import:{image_item_id}:1"),
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

    let first = import(&connection, std::slice::from_ref(&root))
        .await
        .unwrap();
    let mut before = Vec::new();
    for database in ["project", "feature", "item", "run"] {
        before.push(all_rows(&connection, database).await);
    }
    // updated_at は秒の精度なので、同じ秒の中では変わったかを判別できない
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let second = import(&connection, std::slice::from_ref(&root))
        .await
        .unwrap();

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
    assert_eq!(report.skipped.len(), 38);
    assert_every_line_is_accounted(&report);
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

    import(&connection, std::slice::from_ref(&root))
        .await
        .unwrap();

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

    let report = import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();

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
async fn two_roots_with_the_same_repo_are_counted_and_saved_once() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    // 同じ内容の clone / worktree が 2 つ (git の外なので repo はどちらもディレクトリ名の `notes`)
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let roots = [first.path().join("notes"), second.path().join("notes")];
    for root in &roots {
        copy_dir(&fixture_root(), root);
    }

    let report = import(&connection, &roots).await.unwrap();

    assert_eq!(report.counts, fixture_counts());
    assert_eq!(report.files.len(), 8);
    // 2 つ目の worktree の 4 ファイルは feature 名が重なるため取り込まず、全行をどこにも入らなかった行として報告する
    assert_eq!(report.skipped.len(), 38);
    assert_eq!(
        report.unaccounted.len(),
        report.lines.structured + report.lines.body
    );
    assert!(
        report
            .unaccounted
            .iter()
            .all(|line| line.reason.contains("既に")),
        "{:?}",
        report.unaccounted
    );
    assert_eq!(repos(&connection).await, ["notes"]);
    assert_eq!(all_rows(&connection, "feature").await.len(), 4);
    assert_eq!(all_rows(&connection, "item").await.len(), 12);
    assert_eq!(all_rows(&connection, "run").await.len(), 8);
    let note_editor = query_named(&connection, "feature", &["name=note_editor"]).await;
    assert_eq!(
        note_editor[0]["values"]["verification"]["value"],
        "agent-browser, manual"
    );
    assert_eq!(dry_run(&roots).unwrap().counts, report.counts);
}

#[tokio::test]
async fn a_second_file_with_the_same_feature_name_is_skipped_as_a_whole() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("notes");
    copy_dir(&fixture_root(), &root);
    // note_list と同じ feature 名を名乗る別のファイル (項目名も同じ)
    std::fs::create_dir_all(root.join("features/other")).unwrap();
    std::fs::write(
        root.join("features/other/QA.md"),
        "---\nfeature: note_list\n---\n## 1. 別\n\n- [x] **一覧の並び**: 上書きしてはいけない\n",
    )
    .unwrap();

    let report = import(&connection, std::slice::from_ref(&root))
        .await
        .unwrap();

    assert_eq!(report.counts, fixture_counts());
    // 取り込まないファイルの行は、どこにも入らなかった行として全行を報告する
    let unaccounted: Vec<(usize, &str)> = report
        .unaccounted
        .iter()
        .map(|line| {
            assert!(line.file.ends_with("features/other/QA.md"), "{line:?}");
            assert!(line.reason.contains("features/note_list/QA.md"), "{line:?}");
            (line.line, line.content.as_str())
        })
        .collect();
    assert_eq!(
        unaccounted,
        [
            (1, "---"),
            (2, "feature: note_list"),
            (3, "---"),
            (4, "## 1. 別"),
            (5, ""),
            (6, "- [x] **一覧の並び**: 上書きしてはいけない"),
        ]
    );
    let order = query_named(&connection, "item", &["name=一覧の並び"]).await;
    assert_eq!(order.len(), 1);
    assert_eq!(
        order[0]["values"]["expected"]["value"],
        "ノートが更新日時の新しい順に並ぶ"
    );
    // 取り込まなかったファイルが 1 つ目のファイルの feature の verification を消さない
    let note_list = query_named(&connection, "feature", &["name=note_list"]).await;
    assert_eq!(note_list.len(), 1);
    assert_eq!(
        note_list[0]["values"]["verification"]["value"],
        "agent-browser"
    );
}

#[tokio::test]
async fn an_import_run_is_removed_when_the_record_disappears_from_qa_md() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("QA.md");
    let write = |text: &str| std::fs::write(&file, text).unwrap();
    write("## 1. 表示\n\n- [x] **一覧**: 出る\n- [x] **詳細**: 開く\n");
    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();
    assert_eq!(all_rows(&connection, "run").await.len(), 2);
    // `qa record` が書く run (run_id が `import:` で始まらない) は取り込みで消さない
    let detail = query_named(&connection, "item", &["name=詳細"]).await;
    rows::upsert(
        &connection,
        "run",
        &json!({"values": {"run_id": "record:1", "item": [detail[0]["id"]], "result": "ok", "source": "record"}}),
    )
    .await
    .unwrap();
    // 前の版の取り込みが作った `import:<item の行 ID>` の run は、今の版の run が既にあれば重ならないよう消す
    rows::upsert(
        &connection,
        "run",
        &json!({"values": {"run_id": format!("import:{}", detail[0]["id"].as_str().unwrap()), "item": [detail[0]["id"]], "result": "ok", "source": "import"}}),
    )
    .await
    .unwrap();

    // 一覧のチェックを外す
    write("## 1. 表示\n\n- [ ] **一覧**: 出る\n- [x] **詳細**: 開く\n");
    let report = import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();

    assert_eq!(report.counts.run, 1);
    let run_ids: Vec<String> = query_named(&connection, "run", &[])
        .await
        .iter()
        .map(|run| {
            run["values"]["run_id"]["value"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(run_ids.len(), 2);
    assert!(run_ids.contains(&"record:1".to_owned()), "{run_ids:?}");
    assert!(
        run_ids.contains(&format!("import:{}:1", detail[0]["id"].as_str().unwrap())),
        "{run_ids:?}"
    );
}

#[tokio::test]
async fn removing_the_first_of_two_same_name_items_keeps_the_row_of_the_second() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("QA.md");
    let write = |text: &str| std::fs::write(&file, text).unwrap();
    write("## 1. 表示\n\n- [ ] **A**: X\n- [ ] **A**: Y\n");
    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();
    let expected_by_id = |items: Vec<Value>| -> Vec<(String, String)> {
        items
            .iter()
            .map(|item| {
                (
                    item["id"].as_str().unwrap().to_owned(),
                    item["values"]["expected"]["value"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                )
            })
            .collect()
    };
    let before = expected_by_id(query_named(&connection, "item", &["name=A"]).await);

    write("## 1. 表示\n\n- [ ] **A**: Y\n");
    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();

    // Y は自分の行に当たり、X の行 (QA.md から消えた項目) は書き換えない
    assert_eq!(
        expected_by_id(query_named(&connection, "item", &["name=A"]).await),
        before
    );
}

#[tokio::test]
async fn adding_a_same_name_item_before_an_existing_one_keeps_the_existing_row() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("QA.md");
    let write = |text: &str| std::fs::write(&file, text).unwrap();
    write("## 1. 表示\n\n- [ ] **A**: X\n");
    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();
    let x_id = query_named(&connection, "item", &["name=A"]).await[0]["id"].clone();

    write("## 1. 表示\n\n- [ ] **A**: Y\n- [ ] **A**: X\n");
    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();

    let items = query_named(&connection, "item", &["name=A"]).await;
    let x = items
        .iter()
        .find(|item| item["values"]["expected"]["value"] == "X")
        .unwrap();
    assert_eq!(x["id"], x_id);
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn the_result_goes_to_the_run_the_readers_pick_after_moving_a_previous_format_run() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("QA.md");
    // 同じ日付の確認が 2 つ。読み取り側 (`qa verified`・pr-summary) は同じ日付なら後に追加した行を最後とみなす
    std::fs::write(
        &file,
        "## 1. 表示\n\n- [x] **一覧**: 出る\n\n#### 動作確認\n<details>\n\n### **一覧**: 出る\n\n**確認日: 2026-09-01**\n\n**再確認日: 2026-09-01**\n\n</details>\n",
    )
    .unwrap();
    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();
    let item = query_named(&connection, "item", &["name=一覧"]).await;
    let item_id = item[0]["id"].as_str().unwrap();
    let runs = runs_of_item(&connection, item_id).await;
    rows::delete(&connection, "run", runs[1]["id"].as_str().unwrap())
        .await
        .unwrap();
    rows::upsert(
        &connection,
        "run",
        &json!({"id": runs[0]["id"], "values": {"run_id": format!("import:{item_id}")}}),
    )
    .await
    .unwrap();

    for _ in 0..2 {
        import(&connection, std::slice::from_ref(&file))
            .await
            .unwrap();
        // 追加した順で最後の行 (読み取り側が最後とみなす行) に結果があり、ほかの行には無い
        let results: Vec<Value> = query_named(&connection, "run", &[])
            .await
            .iter()
            .map(|run| run["values"]["result"]["value"].clone())
            .collect();
        assert_eq!(results, [Value::Null, json!("ok")]);
    }
}

#[tokio::test]
async fn a_run_of_the_previous_import_format_keeps_its_row_and_added_values() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("QA.md");
    std::fs::write(
        &file,
        "## 1. 表示\n\n- [x] **一覧**: 出る\n\n#### 動作確認\n<details>\n\n### **一覧**: 出る\n\n**確認日: 2026-09-01**\n\n**再確認日: 2026-09-10**\n\n</details>\n",
    )
    .unwrap();
    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();
    // 前の版の取り込みが作った 1 項目 1 run の状態にし、利用者が env を書き足したことにする
    let item = query_named(&connection, "item", &["name=一覧"]).await;
    let item_id = item[0]["id"].as_str().unwrap();
    let runs = runs_of_item(&connection, item_id).await;
    let legacy_id = runs[0]["id"].as_str().unwrap().to_owned();
    rows::delete(&connection, "run", runs[1]["id"].as_str().unwrap())
        .await
        .unwrap();
    rows::upsert(
        &connection,
        "run",
        &json!({"id": legacy_id, "values": {"run_id": format!("import:{item_id}"), "env": "staging"}}),
    )
    .await
    .unwrap();

    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();

    // 前の版の run は消えずに、結果を持つ最後の確認の run になり、書き足した env が残る
    let runs = runs_of_item(&connection, item_id).await;
    let summaries: Vec<(&Value, &Value, &Value, &Value)> = runs
        .iter()
        .map(|run| {
            (
                &run["id"],
                &run["values"]["run_id"]["value"],
                &run["values"]["result"]["value"],
                &run["values"]["env"]["value"],
            )
        })
        .collect();
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].1, &json!(format!("import:{item_id}:1")));
    assert_eq!(summaries[0].2, &Value::Null);
    assert_eq!(
        summaries[1],
        (
            &json!(legacy_id),
            &json!(format!("import:{item_id}:2")),
            &json!("ok"),
            &json!("staging")
        )
    );
}

/// 行 ID が `item_id` の項目の run を `run_id` の順に返す。
async fn runs_of_item(connection: &libsql::Connection, item_id: &str) -> Vec<Value> {
    let mut runs = query_named(connection, "run", &[format!("item={item_id}").as_str()]).await;
    runs.sort_by_key(|run| {
        run["values"]["run_id"]["value"]
            .as_str()
            .unwrap()
            .to_owned()
    });
    runs
}

#[tokio::test]
async fn lossless_fixture_keeps_every_line_and_each_confirmation_becomes_a_run() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let root = lossless_root();

    let report = import(&connection, std::slice::from_ref(&root))
        .await
        .unwrap();

    assert_eq!(
        report.counts,
        ImportCounts {
            project: 1,
            feature: 2,
            item: 4,
            run: 6,
        }
    );
    assert_every_line_is_accounted(&report);
    assert_eq!(
        lines_missing_from(&report, &imported_text(&connection).await),
        Vec::<String>::new()
    );

    // 再確認が 2 回追記された項目は、確認ごとに run が 1 行 (計 3 行)。結果・commit は最後の確認の run だけが持つ
    let send = query_named(&connection, "item", &["name=編集が別の端末に届く"]).await;
    assert_eq!(send.len(), 1);
    assert_eq!(send[0]["values"]["section"]["value"], "1. 送信");
    assert_eq!(send[0]["body"], "- 補足: 端末 B は画面を開いたままにする");
    let send_id = send[0]["id"].as_str().unwrap();
    let send_runs = runs_of_item(&connection, send_id).await;
    let summaries: Vec<Value> = send_runs
        .iter()
        .map(|run| {
            json!({
                "body": run["body"],
                "values": plain_values(run),
            })
        })
        .collect();
    assert_eq!(
        summaries,
        [
            json!({
                "body": "![](<https://example.com/lossless-notes/sync/send-1.png>)\n端末 B の一覧の先頭に出た",
                "values": {
                    "run_id": format!("import:{send_id}:1"),
                    "item": [send_id],
                    "recorded_at": "2026-09-01",
                    "source": "import",
                    "image_url": "https://example.com/lossless-notes/sync/send-1.png",
                },
            }),
            json!({
                "body": "![](<https://example.com/lossless-notes/sync/send-2a.png>)\n![](<https://example.com/lossless-notes/sync/send-2b.png>)\n差分の送り方を変えた後も届いた",
                "values": {
                    "run_id": format!("import:{send_id}:2"),
                    "item": [send_id],
                    "recorded_at": "2026-09-12",
                    "source": "import",
                    "image_url": "https://example.com/lossless-notes/sync/send-2a.png",
                },
            }),
            // 注記付きの確認日: 先頭の日付を recorded_at に、注記を本文に
            json!({
                "body": "（iPad は未確認）\n![](<https://example.com/lossless-notes/sync/send-3.png>)\n```text\nreceived 1 change in 2.1s\n```",
                "values": {
                    "run_id": format!("import:{send_id}:3"),
                    "item": [send_id],
                    "result": "ok",
                    "recorded_at": "2026-09-20",
                    "source": "import",
                    "image_url": "https://example.com/lossless-notes/sync/send-3.png",
                    "commit": "0123456789abcdef0123456789abcdef01234567",
                },
            }),
        ]
    );

    // 形式外の自動化の値は automation_detail と本文に残す。❌ の本文は run の note
    let offline = query_named(&connection, "item", &["name=オフラインの編集を送る"]).await;
    assert!(offline[0]["values"].get("automation").is_none());
    assert_eq!(
        offline[0]["values"]["automation_detail"]["value"],
        "unit（sync_queue_test.rs）"
    );
    let offline_runs = runs_of_item(&connection, offline[0]["id"].as_str().unwrap()).await;
    assert_eq!(offline_runs.len(), 1);
    assert_eq!(offline_runs[0]["values"]["result"]["value"], "ng");
    assert_eq!(
        offline_runs[0]["values"]["note"]["value"],
        "51 件目以降が送られない"
    );

    // 同じ名前の 2 つの項目は別の item として残り、それぞれ自分のエビデンスの run を持つ
    let conflicts = query_named(&connection, "item", &["name=競合を表示する"]).await;
    let mut conflict_summaries = Vec::new();
    for conflict in &conflicts {
        let runs = runs_of_item(&connection, conflict["id"].as_str().unwrap()).await;
        conflict_summaries.push((
            conflict["values"]["expected"]["value"]
                .as_str()
                .unwrap()
                .to_owned(),
            conflict["values"]["section"]["value"]
                .as_str()
                .unwrap()
                .to_owned(),
            runs.len(),
            runs[0]["values"]["image_url"]["value"]
                .as_str()
                .unwrap()
                .to_owned(),
        ));
    }
    assert_eq!(
        conflict_summaries,
        [
            (
                "同じノートを 2 台で編集すると競合の印が出る".to_owned(),
                "2. 競合".to_owned(),
                1,
                "https://example.com/lossless-notes/sync/conflict-mark.png".to_owned()
            ),
            (
                "競合の印を押すと両方の版を並べて見られる".to_owned(),
                "2. 競合".to_owned(),
                1,
                "https://example.com/lossless-notes/sync/conflict-compare.png".to_owned()
            ),
        ]
    );

    // feature の本文: 冒頭の注記の段落・項目の間の地の文・注記のセクションの後ろに、QA.md の原文をそのまま持つ
    for (name, relative) in [("sync", "features/sync/QA.md"), ("_root", "QA.md")] {
        let text = std::fs::read_to_string(root.join(relative)).unwrap();
        let feature = query_named(&connection, "feature", &[format!("name={name}").as_str()]).await;
        let body = feature[0]["body"].as_str().unwrap();
        assert!(body.contains(&text), "{body}");
        assert_eq!(
            body,
            feature_body(&qa_md::parse(relative, &text).body, &text)
        );
    }
    let sync = query_named(&connection, "feature", &["name=sync"]).await;
    let sync_body = sync[0]["body"].as_str().unwrap();
    for paragraph in [
        "前提: テスト用のアカウントを 2 つ用意し、両方の端末でログインしておく。",
        "**2026-09-01 で判明した制約**: オフラインの間の編集は 50 件までしか送られない。",
        "## 1. 送信\n\nこの節の項目は、端末 A と B を同じアカウントでそろえてから確認する。",
        "## 同期の方式の変更に関する注記\n\n2026-09-10 に差分の送り方を変えた。",
    ] {
        assert!(
            sync_body
                .split("## QA.md の原文")
                .next()
                .unwrap()
                .contains(paragraph),
            "{paragraph}"
        );
    }
    let root_feature = query_named(&connection, "feature", &["name=_root"]).await;
    assert!(
        root_feature[0]["body"]
            .as_str()
            .unwrap()
            .starts_with("# QA 全体ガイド\n\nこの QA.md は sample-sync の確認の進め方をまとめる。"),
        "{}",
        root_feature[0]["body"]
    );
}

#[tokio::test]
async fn importing_the_lossless_fixture_twice_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let root = lossless_root();

    let first = import(&connection, std::slice::from_ref(&root))
        .await
        .unwrap();
    let mut before = Vec::new();
    for database in ["project", "feature", "item", "run"] {
        before.push(all_rows(&connection, database).await);
    }
    // updated_at は秒の精度なので、同じ秒の中では変わったかを判別できない
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let second = import(&connection, std::slice::from_ref(&root))
        .await
        .unwrap();

    assert_eq!(second, first);
    let mut after = Vec::new();
    for database in ["project", "feature", "item", "run"] {
        after.push(all_rows(&connection, database).await);
    }
    assert_eq!(after, before);
    assert_eq!(after.iter().map(Vec::len).collect::<Vec<_>>(), [1, 2, 4, 6]);
}

#[tokio::test]
async fn the_result_goes_to_the_latest_confirmation_even_when_dates_are_out_of_order() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("QA.md");
    std::fs::write(
        &file,
        "## 1. 表示\n\n- [x] **一覧**: 出る\n\n#### 動作確認\n<details>\n\n### **一覧**: 出る\n\n**確認日: 2026-09-10**\n\n**再確認日: 2026-09-01**\n\n</details>\n",
    )
    .unwrap();

    import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();

    let item = query_named(&connection, "item", &["name=一覧"]).await;
    let runs = runs_of_item(&connection, item[0]["id"].as_str().unwrap()).await;
    let results: Vec<(&Value, &Value)> = runs
        .iter()
        .map(|run| {
            (
                &run["values"]["recorded_at"]["value"],
                &run["values"]["result"]["value"],
            )
        })
        .collect();
    assert_eq!(
        results,
        [
            (&json!("2026-09-10"), &json!("ok")),
            (&json!("2026-09-01"), &Value::Null),
        ]
    );
}

#[tokio::test]
async fn confirmations_of_an_item_without_a_record_stay_in_the_item_body() {
    let home = tempfile::tempdir().unwrap();
    let connection = open(&home).await;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("QA.md");
    // チェックを外した (再テスト待ちの) 項目のエビデンス。run にすると未検証の項目から外れるため、項目の本文に残す
    std::fs::write(
        &file,
        "## 1. 表示\n\n- [ ] **一覧**: 出る\n  - 自動化: todo\n\n#### 動作確認\n<details>\n\n### **一覧**: 出る\n\n**確認日: 2026-09-10**\n<img src=\"https://example.com/a.png\">\n\n</details>\n",
    )
    .unwrap();

    let report = import(&connection, std::slice::from_ref(&file))
        .await
        .unwrap();

    assert_eq!(report.counts.run, 0);
    assert!(all_rows(&connection, "run").await.is_empty());
    let item = query_named(&connection, "item", &["name=一覧"]).await;
    assert_eq!(
        item[0]["body"],
        "**確認日: 2026-09-10**\n![](<https://example.com/a.png>)"
    );
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
