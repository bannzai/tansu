//! `fixtures/qa/` の手書きの合成 QA.md (`.claude/rules/synthetic-fixtures.md`) をパーサで読み、
//! 項目ごとの期待値と、本文に残した行の報告と、どこにも入らなかった行が無いことを確かめる。

use tansu_core::qa::qa_md::{
    QaAutomation, QaMd, QaMdConfirmation, QaMdItem, QaMdRecord, QaMdRecordKind, QaMdSpecRow,
    SkippedLine, parse,
};

/// `fixtures/qa/<relative>` を読み、(パース結果, ファイルの行数) を返す。
fn parse_fixture(relative: &str) -> (QaMd, usize) {
    let path = format!(
        "{}/../../fixtures/qa/{relative}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap();
    (parse(relative, &text), text.lines().count())
}

/// すべての行が、構造として読んだ行か本文に残した行のどちらか 1 つに数えられていることを確かめる。
fn assert_every_line_is_accounted(qa_md: &QaMd, line_count: usize) {
    assert!(qa_md.unaccounted.is_empty(), "{:?}", qa_md.unaccounted);
    assert_eq!(qa_md.structured_lines + qa_md.skipped.len(), line_count);
}

/// 記録も確認も本文も仕様 ID も無い、未チェックの項目。
fn plain_item(
    line: usize,
    section: &str,
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
        section: Some(section.into()),
        automation,
        automation_detail: detail.map(Into::into),
        record: None,
        confirmations: Vec::new(),
        body: String::new(),
        spec_ids: Vec::new(),
    }
}

/// 確認日 `date` の行 `line` に画像 `image_url` が 1 枚だけある確認。
fn confirmation(line: usize, date: &str, image_url: &str) -> QaMdConfirmation {
    QaMdConfirmation {
        line,
        date: Some(date.into()),
        image_url: Some(image_url.into()),
        body: format!("![](<{image_url}>)"),
    }
}

/// 本文に残した行の行番号。
fn skipped_line_numbers(qa_md: &QaMd) -> Vec<usize> {
    qa_md.skipped.iter().map(|skipped| skipped.line).collect()
}

#[test]
fn root_qa_md_keeps_the_guide_in_the_feature_body() {
    let (qa_md, line_count) = parse_fixture("sample-notes/QA.md");
    assert_eq!(qa_md.feature.as_deref(), Some("_root"));
    assert_eq!(qa_md.verification.as_deref(), Some("agent-browser"));
    assert_eq!(qa_md.last_verified_commit, None);
    assert_eq!(qa_md.last_verified_at, None);
    assert_eq!(
        qa_md.items,
        [
            QaMdItem {
                checked: true,
                confirmations: vec![confirmation(
                    49,
                    "2026-10-05",
                    "https://example.com/sample-notes/root/top.png"
                )],
                ..plain_item(
                    37,
                    "1. 起動",
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
                    "1. 起動",
                    "ログインできる",
                    "テストアカウントでログインするとヘッダーに名前が出る",
                    None,
                    None
                )
            },
        ]
    );
    assert!(qa_md.spec_rows.is_empty());
    // タイトル・対象環境・実行ナレッジ・機能別 QA.md・QA 対象外の散文は、見出しごと feature の本文に残す
    assert_eq!(
        skipped_line_numbers(&qa_md),
        [
            7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 30, 31, 64, 66, 67, 68, 70, 72, 73, 74
        ]
    );
    assert!(
        qa_md.body.starts_with(
            "# QA 全体ガイド\n\n## 対象環境\n\nlocal (ブラウザで http://127.0.0.1:4000 を開く)\n\n## 起動方法\n\n`bun run dev`\n\n"
        ),
        "{}",
        qa_md.body
    );
    assert!(
        qa_md.body.contains(
            "## 実行ナレッジ\n\n### 初回起動が遅い\n\n- 発見日: 2026-10-01\n- 事象: 初回の `bun run dev` は依存の解決で 30 秒かかる\n- 対処: 2 回目以降は数秒で起動する\n\n## 横断確認項目\n\n## 機能別 QA.md\n\n"
        ),
        "{}",
        qa_md.body
    );
    assert!(
        qa_md.body.ends_with(
            "## QA 対象外\n\n| feature | 理由 |\n| --- | --- |\n| `shared` / `theme` | UI を持たない共通コードで、各 feature の項目で確認する |"
        ),
        "{}",
        qa_md.body
    );
    assert_every_line_is_accounted(&qa_md, line_count);
}

#[test]
fn feature_qa_md_reads_evidence_automation_and_spec_checklist() {
    let (qa_md, line_count) = parse_fixture("sample-notes/features/note_list/QA.md");
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
                confirmations: vec![confirmation(
                    44,
                    "2026-10-06",
                    "https://example.com/sample-notes/note_list/order.png"
                )],
                spec_ids: vec!["S1".into()],
                ..plain_item(
                    26,
                    "1. 表示",
                    "一覧の並び",
                    "ノートが更新日時の新しい順に並ぶ",
                    Some(QaAutomation::Auto),
                    Some("e2e/flows/note_list_order.yaml")
                )
            },
            QaMdItem {
                checked: true,
                confirmations: vec![confirmation(
                    53,
                    "2026-10-06",
                    "https://example.com/sample-notes/note_list/search.png"
                )],
                spec_ids: vec!["S2".into()],
                ..plain_item(
                    28,
                    "1. 表示",
                    "検索で絞り込める",
                    "検索語を入れると、含むノートだけが残る",
                    Some(QaAutomation::Manual),
                    Some("検索語の入力タイミングで結果が変わるため目視で確認する")
                )
            },
            plain_item(
                30,
                "1. 表示",
                "空の案内",
                "ノートが 0 件なら「ノートがありません」と表示される",
                Some(QaAutomation::Todo),
                None
            ),
            QaMdItem {
                spec_ids: vec!["S4".into()],
                body: "- 優先度: 高".into(),
                ..plain_item(
                    32,
                    "1. 表示",
                    "ピン留めの固定",
                    "ピン留めしたノートが先頭に固定される",
                    Some(QaAutomation::Todo),
                    None
                )
            },
            plain_item(
                80,
                "2. 操作",
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
    assert_eq!(skipped_line_numbers(&qa_md), [7, 9, 11, 12, 22, 34, 82]);
    let s5 = &qa_md.skipped[4];
    assert_eq!(s5.file, "sample-notes/features/note_list/QA.md");
    assert_eq!(
        s5.content,
        "| S5 | 削除したノートはゴミ箱に移る | ゴミ箱へ移動 |"
    );
    assert!(!s5.reason.is_empty());
    // 仕様チェックリストは対応の無い仕様 (S3) も含めて表ごと feature の本文に残し、読めない項目の行は項目の間の地の文として残す
    assert!(
        qa_md
            .body
            .contains("## 仕様チェックリスト\n\n| ID | 期待挙動 | 対応項目 |\n|----|---------|---------|\n| S1 |"),
        "{}",
        qa_md.body
    );
    assert!(
        qa_md
            .body
            .contains("| S3 | ノートが 0 件なら案内文を表示する | — |"),
        "{}",
        qa_md.body
    );
    assert!(
        qa_md
            .body
            .ends_with("## 2. 操作\n\n- [ ] 太字の無い項目: この行は項目として読めない"),
        "{}",
        qa_md.body
    );
    assert_every_line_is_accounted(&qa_md, line_count);
}

#[test]
fn skip_and_fail_records_keep_their_nested_details() {
    let (qa_md, line_count) = parse_fixture("sample-notes/features/note_editor/QA.md");
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
                confirmations: vec![confirmation(
                    36,
                    "2026-10-07",
                    "https://example.com/sample-notes/note_editor/save.png"
                )],
                ..plain_item(
                    16,
                    "1. 編集",
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
                confirmations: vec![confirmation(
                    45,
                    "2026-10-07",
                    "https://example.com/sample-notes/note_editor/image-drop-failed.png"
                )],
                ..plain_item(
                    18,
                    "1. 編集",
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
                    "1. 編集",
                    "音声入力で書ける",
                    "マイクのボタンを押すと音声が本文になる",
                    Some(QaAutomation::Manual),
                    Some("マイクが要るため")
                )
            },
        ]
    );
    assert_eq!(skipped_line_numbers(&qa_md), [7, 9, 11, 12]);
    assert_every_line_is_accounted(&qa_md, line_count);
}

#[test]
fn broken_lines_are_kept_in_a_body_and_the_rest_is_still_read() {
    let (qa_md, line_count) = parse_fixture("sample-notes/features/settings/QA.md");
    assert_eq!(qa_md.feature.as_deref(), Some("settings"));
    assert_eq!(qa_md.last_verified_commit, None);
    assert_eq!(qa_md.last_verified_at, None);
    assert_eq!(
        qa_md.items,
        [
            QaMdItem {
                checked: true,
                confirmations: vec![QaMdConfirmation {
                    line: 28,
                    date: Some("2026-10-08".into()),
                    image_url: Some("https://example.com/sample-notes/settings/dark.png".into()),
                    // 2 つ目以降の画像も確認の本文に残す
                    body: "![](<https://example.com/sample-notes/settings/dark.png>)\n![](<https://example.com/sample-notes/settings/dark-2.png>)".into(),
                }],
                ..plain_item(
                    16,
                    "1. 表示",
                    "テーマを切り替えられる",
                    "ダークテーマを選ぶと背景が黒になる",
                    Some(QaAutomation::Todo),
                    None
                )
            },
            plain_item(
                18,
                "1. 表示",
                "言語を切り替えられる",
                "日本語と英語を切り替えられる",
                None,
                None
            ),
        ]
    );
    let skipped: Vec<(usize, &str, &str)> = qa_md
        .skipped
        .iter()
        .map(|skipped| {
            (
                skipped.line,
                skipped.content.as_str(),
                skipped.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(
        skipped,
        [
            (6, "owner: team-notes", "frontmatter の知らないキー"),
            (8, "# settings QA", "タイトルの見出し"),
            (10, "## 関連リンク", "散文のセクションの見出し"),
            (
                12,
                "- 仕様: 仕様なし QA",
                "項目にもエビデンスにも当たらない地の文"
            ),
            (
                34,
                "### **通知を切り替えられる**: この見出しに対応する項目は無い",
                "エビデンスの見出しの項目名に一致するチェック項目が無い"
            ),
        ]
    );
    assert_eq!(
        qa_md.body,
        "owner: team-notes\n# settings QA\n\n## 関連リンク\n\n- 仕様: 仕様なし QA\n\n## 1. 表示\n\n### **通知を切り替えられる**: この見出しに対応する項目は無い"
    );
    assert_every_line_is_accounted(&qa_md, line_count);
}

#[test]
fn reconfirmations_duplicate_names_sections_and_notes_are_all_kept() {
    let (qa_md, line_count) = parse_fixture("lossless-notes/features/sync/QA.md");
    assert_eq!(qa_md.items.len(), 4);
    let send = &qa_md.items[0];
    assert_eq!(send.section.as_deref(), Some("1. 送信"));
    assert_eq!(send.body, "- 補足: 端末 B は画面を開いたままにする");
    // 確認日・再確認日ごとに 1 つの確認。画像は確認ごとにすべて、説明文と注記とコードブロックはその確認の本文に入る
    assert_eq!(
        send.confirmations,
        [
            QaMdConfirmation {
                line: 46,
                date: Some("2026-09-01".into()),
                image_url: Some("https://example.com/lossless-notes/sync/send-1.png".into()),
                body: "![](<https://example.com/lossless-notes/sync/send-1.png>)\n端末 B の一覧の先頭に出た".into(),
            },
            QaMdConfirmation {
                line: 50,
                date: Some("2026-09-12".into()),
                image_url: Some("https://example.com/lossless-notes/sync/send-2a.png".into()),
                body: "![](<https://example.com/lossless-notes/sync/send-2a.png>)\n![](<https://example.com/lossless-notes/sync/send-2b.png>)\n差分の送り方を変えた後も届いた".into(),
            },
            QaMdConfirmation {
                line: 54,
                date: Some("2026-09-20".into()),
                image_url: Some("https://example.com/lossless-notes/sync/send-3.png".into()),
                body: "（iPad は未確認）\n![](<https://example.com/lossless-notes/sync/send-3.png>)\n```text\nreceived 1 change in 2.1s\n```".into(),
            },
        ]
    );

    // 形式外の自動化の値は automation_detail と本文に残し、確認日より前の説明文は項目の本文に入る。
    // 「確認日」を含む説明文は確認の始まりにしない
    let offline = &qa_md.items[1];
    assert_eq!(offline.automation, None);
    assert_eq!(
        offline.automation_detail.as_deref(),
        Some("unit（sync_queue_test.rs）")
    );
    assert_eq!(
        offline.body,
        "- 自動化: unit（sync_queue_test.rs）\n\n前回の確認日から送信の上限が変わっていない"
    );
    assert_eq!(offline.confirmations.len(), 1);
    assert_eq!(
        offline.record.as_ref().map(|record| record.note.as_str()),
        Some("51 件目以降が送られない")
    );

    // 同じ名前の項目は別の項目にし、同じ名前の k 番目のエビデンスの見出しを k 番目の項目に当てる
    let conflicts: Vec<(&str, &str, Option<&str>)> = qa_md.items[2..]
        .iter()
        .map(|item| {
            (
                item.name.as_str(),
                item.expected.as_str(),
                item.confirmations[0].image_url.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        conflicts,
        [
            (
                "競合を表示する",
                "同じノートを 2 台で編集すると競合の印が出る",
                Some("https://example.com/lossless-notes/sync/conflict-mark.png")
            ),
            (
                "競合を表示する",
                "競合の印を押すと両方の版を並べて見られる",
                Some("https://example.com/lossless-notes/sync/conflict-compare.png")
            ),
        ]
    );
    assert_eq!(qa_md.items[2].section.as_deref(), Some("2. 競合"));
    // 画像・summary の行に併記された文字は、タグを除いて本文に残す
    assert_eq!(qa_md.items[2].body, "動作確認スクショ (端末 A と B)");
    assert_eq!(
        qa_md.items[2].confirmations[0].body,
        "![](<https://example.com/lossless-notes/sync/conflict-mark.png>)\n競合の印はノートの右上に出た"
    );
    // 項目の入れ子のコードブロックは項目の本文に、1 段浅くして入る
    assert_eq!(
        qa_md.items[3].body,
        "```sh\nsample-sync conflict --note demo\n```"
    );

    // 冒頭の段落・項目の間の地の文 (そのセクションの見出しを添える)・注記のセクションは feature の本文に入る
    assert_eq!(
        qa_md.body,
        "# sync QA\n\n前提: テスト用のアカウントを 2 つ用意し、両方の端末でログインしておく。\n\n**2026-09-01 で判明した制約**: オフラインの間の編集は 50 件までしか送られない。\n\n## 1. 送信\n\nこの節の項目は、端末 A と B を同じアカウントでそろえてから確認する。\n\n## 同期の方式の変更に関する注記\n\n2026-09-10 に差分の送り方を変えた。それより前の確認日の画像は旧方式の画面。"
    );
    assert_every_line_is_accounted(&qa_md, line_count);
}

#[test]
fn root_guide_with_a_code_block_is_kept_in_the_feature_body() {
    let (qa_md, line_count) = parse_fixture("lossless-notes/QA.md");
    assert!(qa_md.items.is_empty());
    assert!(
        qa_md
            .body
            .contains("## 起動方法\n\n```sh\nbun run dev --port 4100\n```\n\n## 実行ナレッジ"),
        "{}",
        qa_md.body
    );
    assert_every_line_is_accounted(&qa_md, line_count);
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
    assert_eq!(qa_md.items[0].body, "```sh\n- [x] **別**: 例\n```");
    assert_eq!(
        qa_md.body,
        "## 実行ナレッジ\n\n```md\n- [x] **一覧**: 記述例\n## 1. 例\n```"
    );
    assert_every_line_is_accounted(&qa_md, 13);
}

#[test]
fn an_evidence_heading_goes_to_the_item_with_the_same_name_and_expected_behavior() {
    // 同じ名前の前の項目にはエビデンスが無い。見出しの期待動作で後ろの項目に当てる
    let qa_md = parse(
        "x/QA.md",
        "- [ ] **A**: X\n- [x] **A**: Y\n\n#### 動作確認\n<details>\n\n### **A**: Y\n\n**確認日: 2026-10-01**\n\n### **A**: 古い文\n\n**確認日: 2026-10-02**\n\n</details>\n",
    );
    let dates: Vec<Vec<Option<&str>>> = qa_md
        .items
        .iter()
        .map(|item| {
            item.confirmations
                .iter()
                .map(|confirmation| confirmation.date.as_deref())
                .collect()
        })
        .collect();
    // 期待動作が一致しない見出しは、同じ名前の k 番目の項目 (ここでは 1 番目の X) に当て、見出しの行をその項目の本文に残す
    assert_eq!(dates, [vec![Some("2026-10-02")], vec![Some("2026-10-01")]]);
    assert_eq!(qa_md.items[0].body, "### **A**: 古い文");
    assert_eq!(skipped_line_numbers(&qa_md), [11]);
    assert_every_line_is_accounted(&qa_md, 15);
}

#[test]
fn an_item_section_without_items_keeps_its_heading_in_the_feature_body() {
    let qa_md = parse(
        "x/QA.md",
        "## 横断確認項目\n\n## 1. 表示\n\n- [ ] **一覧**: 出る\n\n## 2. 操作\n",
    );
    assert_eq!(qa_md.body, "## 横断確認項目\n\n## 2. 操作");
    assert_eq!(qa_md.items[0].section.as_deref(), Some("1. 表示"));
    assert_every_line_is_accounted(&qa_md, 7);
}

#[test]
fn evidence_descriptions_keep_their_indentation_and_trailing_spaces() {
    let qa_md = parse(
        "x/QA.md",
        "- [x] **一覧**: 出る\n\n#### 動作確認\n<details>\n\n### **一覧**: 出る\n\n**確認日: 2026-10-09**\n- 手順  \n  - 補足\n\n</details>\n",
    );
    assert_eq!(qa_md.items[0].confirmations[0].body, "- 手順  \n  - 補足");
    assert_every_line_is_accounted(&qa_md, 12);
}

#[test]
fn spec_rows_whose_id_cannot_be_an_option_are_reported() {
    let qa_md = parse(
        "x/QA.md",
        "## 仕様チェックリスト\n\n| ID | 期待挙動 | 対応項目 |\n|---|---|---|\n| | 空の ID | 一覧 |\n| S1,S2 | カンマ入り | 一覧 |\n| S3 | 正常 | 一覧 |\n\n## 1. 表示\n\n- [ ] **一覧**: 出る\n",
    );
    assert_eq!(qa_md.items[0].spec_ids, ["S3"]);
    assert_eq!(skipped_line_numbers(&qa_md), [5, 6]);
    assert!(qa_md.body.contains("| S1,S2 | カンマ入り | 一覧 |"));
    assert_every_line_is_accounted(&qa_md, 11);
}

#[test]
fn an_evidence_block_without_details_ends_at_the_next_heading() {
    let qa_md = parse(
        "x/QA.md",
        "## 1. 表示\n\n- [x] **一覧**: 出る\n\n#### 動作確認\n\n### **一覧**: 出る\n\n**確認日: 2026-10-09**\n\n## 2. 操作\n\n- [ ] **削除**: 消える\n",
    );
    assert_eq!(qa_md.items.len(), 2);
    assert_eq!(
        qa_md.items[0].confirmations[0].date.as_deref(),
        Some("2026-10-09")
    );
    assert_eq!(qa_md.items[1].name, "削除");
    assert!(qa_md.skipped.is_empty(), "{:?}", qa_md.skipped);
    assert_every_line_is_accounted(&qa_md, 13);
}

#[test]
fn code_fences_close_only_by_the_commonmark_rule() {
    let qa_md = parse(
        "x/QA.md",
        "## 1. 表示\n\n````md\n```sh\n- [x] **例 1**: 4 つのフェンスの中\n```\n````\n~~~\n- [x] **例 2**: チルダのフェンスの中\n~~~\n\n- [ ] **一覧**: 出る\n",
    );
    assert_eq!(qa_md.items.len(), 1);
    assert_eq!(qa_md.items[0].name, "一覧");
    assert_eq!(skipped_line_numbers(&qa_md), [3, 4, 5, 6, 7, 8, 9, 10]);
    assert_every_line_is_accounted(&qa_md, 12);
}

#[test]
fn unreadable_dates_are_kept_in_a_body() {
    let qa_md = parse(
        "x/QA.md",
        "---\nfeature: f\nlast_verified_at: いつか\n---\n- [x] **一覧**: 出る\n\n#### 動作確認\n\n### **一覧**: 出る\n\n**確認日: 未定**\n",
    );
    assert_eq!(qa_md.last_verified_at, None);
    assert_eq!(
        qa_md.skipped,
        [
            SkippedLine {
                file: "x/QA.md".into(),
                line: 3,
                content: "last_verified_at: いつか".into(),
                reason: "last_verified_at が YYYY-MM-DD で読めない".into(),
            },
            SkippedLine {
                file: "x/QA.md".into(),
                line: 11,
                content: "**確認日: 未定**".into(),
                reason: "確認日の日付が YYYY-MM-DD で読めない".into(),
            },
        ]
    );
    assert_eq!(
        qa_md.items[0].confirmations,
        [QaMdConfirmation {
            line: 11,
            date: None,
            image_url: None,
            body: "**確認日: 未定**".into(),
        }]
    );
    assert_eq!(qa_md.body, "last_verified_at: いつか");
    assert_every_line_is_accounted(&qa_md, 11);
}
