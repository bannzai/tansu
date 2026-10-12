//! `fixtures/qa/sample-notes/` (手書きの合成 QA.md。`.claude/rules/synthetic-fixtures.md`) をパーサで読み、
//! 項目ごとの期待値と、読み飛ばした行の報告を確かめる。

use tansu_core::qa::qa_md::{
    QaAutomation, QaMd, QaMdItem, QaMdRecord, QaMdRecordKind, QaMdSpecRow, SkippedLine, parse,
};

/// `fixtures/qa/sample-notes/<relative>` を読む。
fn parse_fixture(relative: &str) -> QaMd {
    let path = format!(
        "{}/../../fixtures/qa/sample-notes/{relative}",
        env!("CARGO_MANIFEST_DIR")
    );
    parse(relative, &std::fs::read_to_string(&path).unwrap())
}

/// 記録も確認日も画像も仕様 ID も無い、未チェックの項目。
fn plain_item(
    line: usize,
    name: &str,
    expected: &str,
    automation: Option<QaAutomation>,
    detail: Option<&str>,
) -> QaMdItem {
    QaMdItem {
        line,
        name: name.into(),
        expected: expected.into(),
        checked: false,
        automation,
        automation_detail: detail.map(Into::into),
        record: None,
        verified_at: None,
        image_url: None,
        spec_ids: Vec::new(),
    }
}

/// 読み飛ばした行を (行番号, 内容) にする。
fn skipped_lines(qa_md: &QaMd) -> Vec<(usize, &str)> {
    qa_md
        .skipped
        .iter()
        .map(|skipped| (skipped.line, skipped.content.as_str()))
        .collect()
}

#[test]
fn root_qa_md_reads_cross_cutting_items_without_automation() {
    let qa_md = parse_fixture("QA.md");
    assert_eq!(qa_md.feature.as_deref(), Some("_root"));
    assert_eq!(qa_md.verification.as_deref(), Some("agent-browser"));
    assert_eq!(qa_md.last_verified_commit, None);
    assert_eq!(qa_md.last_verified_at, None);
    assert_eq!(
        qa_md.items,
        [
            QaMdItem {
                checked: true,
                verified_at: Some("2026-10-05".into()),
                image_url: Some("https://example.com/sample-notes/root/top.png".into()),
                ..plain_item(
                    37,
                    "トップページが開く",
                    "`/` を開くとノートの一覧が表示される",
                    None,
                    None
                )
            },
            QaMdItem {
                record: Some(QaMdRecord {
                    kind: QaMdRecordKind::Skip,
                    note: "テストアカウントの発行待ち".into(),
                }),
                ..plain_item(
                    38,
                    "ログインできる",
                    "テストアカウントでログインするとヘッダーに名前が出る",
                    None,
                    None
                )
            },
        ]
    );
    assert!(qa_md.spec_rows.is_empty());
    // 対象環境・実行ナレッジ・機能別 QA.md・QA 対象外の散文は読み飛ばした行として報告しない
    assert!(qa_md.skipped.is_empty(), "{:?}", qa_md.skipped);
}

#[test]
fn feature_qa_md_reads_evidence_automation_and_spec_checklist() {
    let qa_md = parse_fixture("features/note_list/QA.md");
    assert_eq!(qa_md.feature.as_deref(), Some("note_list"));
    assert_eq!(
        qa_md.last_verified_commit.as_deref(),
        Some("3f2a9c1e5b7d4a6f8c0e2b4d6f8a1c3e5b7d9f0a")
    );
    assert_eq!(qa_md.last_verified_at.as_deref(), Some("2026-10-06"));
    assert_eq!(
        qa_md.items,
        [
            QaMdItem {
                checked: true,
                verified_at: Some("2026-10-06".into()),
                image_url: Some("https://example.com/sample-notes/note_list/order.png".into()),
                spec_ids: vec!["S1".into()],
                ..plain_item(
                    26,
                    "一覧の並び",
                    "ノートが更新日時の新しい順に並ぶ",
                    Some(QaAutomation::Auto),
                    Some("e2e/flows/note_list_order.yaml")
                )
            },
            QaMdItem {
                checked: true,
                verified_at: Some("2026-10-06".into()),
                image_url: Some("https://example.com/sample-notes/note_list/search.png".into()),
                spec_ids: vec!["S2".into()],
                ..plain_item(
                    28,
                    "検索で絞り込める",
                    "検索語を入れると、含むノートだけが残る",
                    Some(QaAutomation::Manual),
                    Some("検索語の入力タイミングで結果が変わるため目視で確認する")
                )
            },
            plain_item(
                30,
                "空の案内",
                "ノートが 0 件なら「ノートがありません」と表示される",
                Some(QaAutomation::Todo),
                None
            ),
            QaMdItem {
                spec_ids: vec!["S4".into()],
                ..plain_item(
                    32,
                    "ピン留めの固定",
                    "ピン留めしたノートが先頭に固定される",
                    Some(QaAutomation::Todo),
                    None
                )
            },
            plain_item(
                80,
                "スワイプで削除",
                "一覧の行を左にスワイプすると削除ボタンが出る",
                Some(QaAutomation::Auto),
                Some("e2e/flows/note_list_swipe.yaml")
            ),
        ]
    );
    assert_eq!(
        qa_md.spec_rows,
        [
            QaMdSpecRow {
                line: 18,
                id: "S1".into(),
                expected: "ノートは更新日時の新しい順に並ぶ".into(),
                item_name: Some("一覧の並び".into()),
            },
            QaMdSpecRow {
                line: 19,
                id: "S2".into(),
                expected: "検索語を含むノートだけが表示される".into(),
                item_name: Some("検索で絞り込める".into()),
            },
            QaMdSpecRow {
                line: 20,
                id: "S3".into(),
                expected: "ノートが 0 件なら案内文を表示する".into(),
                item_name: None,
            },
            QaMdSpecRow {
                line: 21,
                id: "S4".into(),
                expected: "ピン留めしたノートは先頭に固定される".into(),
                item_name: Some("ピン留めの固定".into()),
            },
            QaMdSpecRow {
                line: 22,
                id: "S5".into(),
                expected: "削除したノートはゴミ箱に移る".into(),
                item_name: Some("ゴミ箱へ移動".into()),
            },
        ]
    );
    assert_eq!(
        skipped_lines(&qa_md),
        [
            (34, "  - 優先度: 高"),
            (82, "- [ ] 太字の無い項目: この行は項目として読めない"),
            (22, "| S5 | 削除したノートはゴミ箱に移る | ゴミ箱へ移動 |"),
        ]
    );
    assert_eq!(qa_md.skipped[0].file, "features/note_list/QA.md");
    assert!(!qa_md.skipped[0].reason.is_empty());
}

#[test]
fn skip_and_fail_records_keep_their_nested_details() {
    let qa_md = parse_fixture("features/note_editor/QA.md");
    assert_eq!(qa_md.verification.as_deref(), Some("agent-browser, manual"));
    assert_eq!(
        qa_md.last_verified_commit.as_deref(),
        Some("a1b2c3d4e5f60718293a4b5c6d7e8f90ab12cd34")
    );
    assert_eq!(qa_md.last_verified_at.as_deref(), Some("2026-10-07"));
    assert_eq!(
        qa_md.items,
        [
            QaMdItem {
                checked: true,
                verified_at: Some("2026-10-07".into()),
                image_url: Some("https://example.com/sample-notes/note_editor/save.png".into()),
                ..plain_item(
                    16,
                    "本文を保存できる",
                    "本文を書いて保存すると一覧に反映される",
                    Some(QaAutomation::Auto),
                    Some("e2e/flows/note_editor_save.yaml")
                )
            },
            QaMdItem {
                record: Some(QaMdRecord {
                    kind: QaMdRecordKind::Fail,
                    note: "ドロップ後に本文が空になる。再現手順: 画像を本文へドロップ → 保存。issue: https://example.com/sample-notes/issues/21".into(),
                }),
                verified_at: Some("2026-10-07".into()),
                image_url: Some(
                    "https://example.com/sample-notes/note_editor/image-drop-failed.png".into()
                ),
                ..plain_item(
                    18,
                    "画像を貼れる",
                    "画像をドロップすると本文に埋め込まれる",
                    Some(QaAutomation::Manual),
                    Some("ドロップ操作を E2E で再現できないため")
                )
            },
            QaMdItem {
                record: Some(QaMdRecord {
                    kind: QaMdRecordKind::Skip,
                    note: "シミュレータではマイクを使えない。実機で確認する\n\
                           起動条件: ローカル sim-boot。SIMSLIM_EXCEPT=default / SLIM_STATUS=applied\n\
                           デーモン: `xcrun simctl spawn <UDID> launchctl list | grep -e com.example.audiod` の出力: 該当なし\n\
                           試した手順: マイクのボタンを押す → 権限のダイアログが出ない。設定アプリから権限を付ける → 項目が無い"
                        .into(),
                }),
                ..plain_item(
                    21,
                    "音声入力で書ける",
                    "マイクのボタンを押すと音声が本文になる",
                    Some(QaAutomation::Manual),
                    Some("マイクが要るため")
                )
            },
        ]
    );
    assert!(qa_md.skipped.is_empty(), "{:?}", qa_md.skipped);
}

#[test]
fn broken_lines_are_reported_and_the_rest_is_still_read() {
    let qa_md = parse_fixture("features/settings/QA.md");
    assert_eq!(qa_md.feature.as_deref(), Some("settings"));
    assert_eq!(qa_md.last_verified_commit, None);
    assert_eq!(qa_md.last_verified_at, None);
    assert_eq!(
        qa_md.items,
        [
            QaMdItem {
                checked: true,
                verified_at: Some("2026-10-08".into()),
                image_url: Some("https://example.com/sample-notes/settings/dark.png".into()),
                ..plain_item(
                    16,
                    "テーマを切り替えられる",
                    "ダークテーマを選ぶと背景が黒になる",
                    Some(QaAutomation::Todo),
                    None
                )
            },
            plain_item(
                18,
                "言語を切り替えられる",
                "日本語と英語を切り替えられる",
                None,
                None
            ),
        ]
    );
    assert_eq!(
        qa_md.skipped,
        [
            SkippedLine {
                file: "features/settings/QA.md".into(),
                line: 6,
                content: "owner: team-notes".into(),
                reason: "frontmatter の知らないキー".into(),
            },
            SkippedLine {
                file: "features/settings/QA.md".into(),
                line: 19,
                content: "- [ ] **テーマを切り替えられる**: 同じ名前の項目 (2 つ目は読み飛ばす)".into(),
                reason: "同じ名前の項目が既にある".into(),
            },
            SkippedLine {
                file: "features/settings/QA.md".into(),
                line: 20,
                content: "  - 自動化: todo".into(),
                reason: "解釈できない行".into(),
            },
            SkippedLine {
                file: "features/settings/QA.md".into(),
                line: 32,
                content: r#"<img src="https://example.com/sample-notes/settings/dark-2.png" width="320">"#.into(),
                reason: "2 つ目以降の画像".into(),
            },
            SkippedLine {
                file: "features/settings/QA.md".into(),
                line: 36,
                content: "### **通知を切り替えられる**: この見出しに対応する項目は無い".into(),
                reason: "エビデンスの見出しの項目名に一致するチェック項目が無い".into(),
            },
        ]
    );
}

#[test]
fn fenced_code_blocks_are_not_read_as_syntax() {
    let qa_md = parse(
        "x/QA.md",
        "## 実行ナレッジ\n\n```md\n- [x] **一覧**: 記述例\n## 1. 例\n```\n\n## 1. 表示\n\n- [ ] **一覧**: 出る\n  ```sh\n  - [x] **別**: 例\n  ```\n",
    );
    assert_eq!(qa_md.items.len(), 1);
    assert_eq!(qa_md.items[0].name, "一覧");
    assert!(!qa_md.items[0].checked);
    assert!(qa_md.skipped.is_empty(), "{:?}", qa_md.skipped);
}

#[test]
fn spec_rows_whose_id_cannot_be_an_option_are_reported() {
    let qa_md = parse(
        "x/QA.md",
        "## 仕様チェックリスト\n\n| ID | 期待挙動 | 対応項目 |\n|---|---|---|\n| | 空の ID | 一覧 |\n| S1,S2 | カンマ入り | 一覧 |\n| S3 | 正常 | 一覧 |\n\n## 1. 表示\n\n- [ ] **一覧**: 出る\n",
    );
    assert_eq!(qa_md.items[0].spec_ids, ["S3"]);
    assert_eq!(
        skipped_lines(&qa_md),
        [
            (5, "| | 空の ID | 一覧 |"),
            (6, "| S1,S2 | カンマ入り | 一覧 |")
        ]
    );
}

#[test]
fn an_evidence_block_without_details_ends_at_the_next_heading() {
    let qa_md = parse(
        "x/QA.md",
        "## 1. 表示\n\n- [x] **一覧**: 出る\n\n#### 動作確認\n\n### **一覧**: 出る\n\n**確認日: 2026-10-09**\n\n## 2. 操作\n\n- [ ] **削除**: 消える\n",
    );
    assert_eq!(qa_md.items.len(), 2);
    assert_eq!(qa_md.items[0].verified_at.as_deref(), Some("2026-10-09"));
    assert_eq!(qa_md.items[1].name, "削除");
    assert!(qa_md.skipped.is_empty(), "{:?}", qa_md.skipped);
}

#[test]
fn code_fences_close_only_by_the_commonmark_rule() {
    let qa_md = parse(
        "x/QA.md",
        "## 1. 表示\n\n````md\n```sh\n- [x] **例 1**: 4 つのフェンスの中\n```\n````\n~~~\n- [x] **例 2**: チルダのフェンスの中\n~~~\n\n- [ ] **一覧**: 出る\n",
    );
    assert_eq!(qa_md.items.len(), 1);
    assert_eq!(qa_md.items[0].name, "一覧");
    assert!(qa_md.skipped.is_empty(), "{:?}", qa_md.skipped);
}

#[test]
fn unreadable_last_verified_at_is_reported_and_dropped() {
    let qa_md = parse(
        "x/QA.md",
        "---\nfeature: f\nlast_verified_at: いつか\n---\n",
    );
    assert_eq!(qa_md.last_verified_at, None);
    assert_eq!(skipped_lines(&qa_md), [(3, "last_verified_at: いつか")]);
}
