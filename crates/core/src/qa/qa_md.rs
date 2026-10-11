//! QA.md (castle の setup-qa / run-qa skill の形式。SSOT は castle の `setup-qa/references/qa-md-format.md`) のパーサ。
//! 理解できない行で失敗せず、読み飛ばした行を [`QaMd::skipped`] に残す (`documents/PROJECT.md`「QA.md の形式」)。

use serde::Serialize;

use crate::named_json::normalize_date;

/// QA.md 1 ファイルを読んだ結果。frontmatter の 4 項目と、チェック項目・仕様チェックリスト・読み飛ばした行を持つ。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QaMd {
    /// frontmatter の `feature` (root QA.md は `_root`)。無い・`null` なら `None`。
    pub feature: Option<String>,
    /// frontmatter の `verification` (動作確認手段)。
    pub verification: Option<String>,
    /// frontmatter の `last_verified_commit` (最終確認時の full SHA)。`null` なら `None`。
    pub last_verified_commit: Option<String>,
    /// frontmatter の `last_verified_at` (`YYYY-MM-DD`)。`null` なら `None`。
    pub last_verified_at: Option<String>,
    /// チェック項目。ファイルに出た順。同じ名前の 2 つ目以降は読み飛ばした行になる。
    pub items: Vec<QaMdItem>,
    /// 仕様チェックリストの表の行。対応項目は [`QaMdItem::spec_ids`] にも写してある。
    pub spec_rows: Vec<QaMdSpecRow>,
    /// 解釈できずに読み飛ばした行。
    pub skipped: Vec<SkippedLine>,
}

/// QA.md のチェック項目 1 つ (`- [ ] **項目名**: 期待動作` と、その入れ子・エビデンス)。
#[derive(Debug, Clone, PartialEq)]
pub struct QaMdItem {
    /// 項目の行番号 (1 始まり)。
    pub line: usize,
    /// `**項目名**` の中。
    pub name: String,
    /// `**項目名**:` の後ろの期待動作。
    pub expected: String,
    /// `- [x]` なら true。
    pub checked: bool,
    /// 入れ子の `自動化:` の種別。行が無ければ `None`。
    pub automation: Option<QaAutomation>,
    /// `自動化: auto(...)` / `manual(...)` の括弧の中 (flow のパス、自動化できない理由)。
    pub automation_detail: Option<String>,
    /// 入れ子の `⏭️ スキップ:` / `❌ 失敗:` の記録。項目の入れ子に無ければエビデンスブロックの記録。
    pub record: Option<QaMdRecord>,
    /// エビデンスブロックの `**確認日: YYYY-MM-DD**` の日付。
    pub verified_at: Option<String>,
    /// エビデンスブロックの `<img src="...">` の URL (最初の 1 つ)。
    pub image_url: Option<String>,
    /// 仕様チェックリストでこの項目を対応項目にしている仕様 ID (S1, S2, ...)。表に出た順。
    pub spec_ids: Vec<String>,
}

/// 入れ子の `自動化:` の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QaAutomation {
    /// `auto(<実行形のパス>)`。E2E で自動化済み。
    Auto,
    /// `manual(<理由>)`。agent の操作・目視で確認する。
    Manual,
    /// `todo`。未カバー。
    Todo,
}

impl QaAutomation {
    /// データベース `item` の select `automation` の選択肢の名前。
    pub fn name(self) -> &'static str {
        match self {
            QaAutomation::Auto => "auto",
            QaAutomation::Manual => "manual",
            QaAutomation::Todo => "todo",
        }
    }
}

/// 項目の `⏭️ スキップ:` / `❌ 失敗:` の記録。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaMdRecord {
    /// スキップか失敗か。
    pub kind: QaMdRecordKind,
    /// 記録の本文。`:` の後ろの理由と、その下に入れ子で書かれた併記事項 (起動条件・デーモン・試した手順・再現手順等) を改行でつないだもの。
    pub note: String,
}

/// `⏭️ スキップ:` と `❌ 失敗:` の区別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QaMdRecordKind {
    /// `⏭️ スキップ:`。
    Skip,
    /// `❌ 失敗:`。
    Fail,
}

/// 仕様チェックリストの表の 1 行 (`| S1 | 期待挙動 | 対応項目 |`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaMdSpecRow {
    /// 仕様 ID (S1, S2, ...)。
    pub id: String,
    /// 期待挙動。
    pub expected: String,
    /// 対応項目の項目名。`—` (テスト漏れ) なら `None`。
    pub item_name: Option<String>,
}

/// 解釈できずに読み飛ばした行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedLine {
    /// 行があるファイル (`parse` に渡した表示名)。
    pub file: String,
    /// 行番号 (1 始まり)。
    pub line: usize,
    /// 行の内容。
    pub content: String,
    /// 読み飛ばした理由。
    pub reason: String,
}

/// `#### 動作確認` の details の中を読んでいる間の状態。
struct Evidence {
    /// `<details>` の深さ。0 に戻ったらブロックの終わり。
    depth: i32,
    /// 最初の `<details>` を見たか (見る前は深さ 0 でも終わりにしない)。
    started: bool,
    /// 直前の `### **項目名**` が指す項目の `items` の添字。項目に無い見出しの下は `None`。
    heading_item: Option<usize>,
}

/// `## ` 見出しで決まる、今いるセクションの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// 見出しの前。
    None,
    /// `## N. 名前` と `## 横断確認項目`。チェック項目を書く場所で、解釈できない行は読み飛ばした行として報告する。
    Items,
    /// `## 仕様チェックリスト`。
    Spec,
    /// それ以外 (`## 関連リンク`、root QA.md の対象環境・起動方法・実行ナレッジ等)。散文なので、解釈できなくても報告しない。
    Prose,
}

/// `file` (表示名) の内容 `text` を読む。失敗せず、解釈できない行は [`QaMd::skipped`] に残す。
///
/// 読む構文: frontmatter (`feature` / `verification` / `last_verified_commit` / `last_verified_at`)、`## N. セクション`、
/// `- [ ] **項目名**: 期待動作` と `- [x]`、入れ子の `自動化: auto(...) | manual(...) | todo` (括弧は全角・半角のどちらでも)、
/// `⏭️ スキップ:` / `❌ 失敗:` とその下の入れ子 (併記事項)、`## 仕様チェックリスト` の表、`#### 動作確認` の details の中の
/// `### **項目名**: 期待動作` と `**確認日: YYYY-MM-DD**`・`<img src="URL">`。
/// 同じ名前の項目が 2 つ以上あれば、2 つ目以降は読み飛ばした行にする (項目は feature と名前で特定するため)。
pub fn parse(file: &str, text: &str) -> QaMd {
    let mut qa_md = QaMd::default();
    let mut section = Section::None;
    let mut in_frontmatter = false;
    let mut current_item: Option<(usize, usize)> = None;
    let mut record_indent: Option<usize> = None;
    let mut evidence: Option<Evidence> = None;
    let skip = |qa_md: &mut QaMd, line_number: usize, content: &str, reason: &str| {
        qa_md.skipped.push(SkippedLine {
            file: file.to_owned(),
            line: line_number,
            content: content.to_owned(),
            reason: reason.to_owned(),
        });
    };

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();

        if index == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            let Some((key, value)) = trimmed.split_once(':') else {
                skip(
                    &mut qa_md,
                    line_number,
                    line,
                    "frontmatter の行が `キー: 値` の形でない",
                );
                continue;
            };
            let value = match value.trim() {
                "" | "null" => None,
                value => Some(value.to_owned()),
            };
            match key.trim() {
                "feature" => qa_md.feature = value,
                "verification" => qa_md.verification = value,
                "last_verified_commit" => qa_md.last_verified_commit = value,
                // run の recorded_at (date) に写すため、日付として読めない値は捨てて報告する
                "last_verified_at" => match value.as_deref().map(normalize_date) {
                    Some(None) => skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "last_verified_at が YYYY-MM-DD で読めない",
                    ),
                    date => qa_md.last_verified_at = date.flatten(),
                },
                _ => skip(&mut qa_md, line_number, line, "frontmatter の知らないキー"),
            }
            continue;
        }

        if let Some(state) = evidence.as_mut() {
            let opens = line.matches("<details").count();
            let closes = line.matches("</details>").count();
            if let Some(heading) = line.strip_prefix("### ") {
                match parse_bold_name(heading.trim()) {
                    Some((name, _)) => {
                        state.heading_item = qa_md.items.iter().position(|item| item.name == name);
                        if state.heading_item.is_none() {
                            skip(
                                &mut qa_md,
                                line_number,
                                line,
                                "エビデンスの見出しの項目名に一致するチェック項目が無い",
                            );
                        }
                    }
                    None => {
                        state.heading_item = None;
                        skip(
                            &mut qa_md,
                            line_number,
                            line,
                            "エビデンスの見出しが `### **項目名**: 期待動作` の形でない",
                        );
                    }
                }
            } else if line.contains("確認日") {
                match (state.heading_item, verified_date(line)) {
                    (Some(item_index), Some(date)) => {
                        let item = &mut qa_md.items[item_index];
                        if item.verified_at.is_none() {
                            item.verified_at = Some(date);
                        } else {
                            skip(&mut qa_md, line_number, line, "2 つ目以降の確認日");
                        }
                    }
                    (Some(_), None) => skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "確認日の日付が YYYY-MM-DD で読めない",
                    ),
                    (None, _) => skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "どの項目の確認日か分からない (エビデンスの見出しの下に無い)",
                    ),
                }
            } else if line.contains("<img") {
                match (state.heading_item, image_source(line)) {
                    (Some(item_index), Some(url)) => {
                        let item = &mut qa_md.items[item_index];
                        if item.image_url.is_none() {
                            item.image_url = Some(url);
                        } else {
                            skip(&mut qa_md, line_number, line, "2 つ目以降の画像");
                        }
                    }
                    (Some(_), None) => skip(&mut qa_md, line_number, line, "画像の src が無い"),
                    (None, _) => skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "どの項目の画像か分からない (エビデンスの見出しの下に無い)",
                    ),
                }
            } else if let Some(record) = parse_record(trimmed.trim_start_matches("- ").trim()) {
                match state.heading_item {
                    Some(item_index) if qa_md.items[item_index].record.is_none() => {
                        qa_md.items[item_index].record = Some(record);
                    }
                    Some(_) => skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "項目に既にスキップ / 失敗の記録がある",
                    ),
                    None => skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "どの項目の記録か分からない (エビデンスの見出しの下に無い)",
                    ),
                }
            } else if !(trimmed.is_empty()
                || trimmed == "（未実行）"
                || trimmed == "(未実行)"
                || trimmed.starts_with("<details")
                || trimmed.starts_with("</details>")
                || trimmed.starts_with("<summary"))
            {
                skip(
                    &mut qa_md,
                    line_number,
                    line,
                    "エビデンスの中で解釈できない行",
                );
            }
            state.depth += opens as i32 - closes as i32;
            if opens > 0 {
                state.started = true;
            }
            if state.started && state.depth <= 0 {
                evidence = None;
            }
            continue;
        }

        if trimmed == "#### 動作確認" {
            evidence = Some(Evidence {
                depth: 0,
                started: false,
                heading_item: None,
            });
            current_item = None;
            record_indent = None;
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            let heading = heading.trim();
            section = if heading == "仕様チェックリスト" {
                Section::Spec
            } else if heading == "横断確認項目" || is_numbered_heading(heading) {
                Section::Items
            } else {
                Section::Prose
            };
            current_item = None;
            record_indent = None;
            continue;
        }
        if trimmed.is_empty() || trimmed == "---" || line.starts_with("# ") {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        if let Some(record_start) = record_indent {
            if indent > record_start {
                if let Some((item_index, _)) = current_item
                    && let Some(record) = qa_md.items[item_index].record.as_mut()
                {
                    record.note.push('\n');
                    record.note.push_str(trimmed.strip_prefix("- ").unwrap_or(trimmed));
                }
                continue;
            }
            record_indent = None;
        }

        if let Some(checkbox) = parse_checkbox(trimmed) {
            let (checked, rest) = checkbox;
            match parse_bold_name(rest) {
                Some((name, expected)) => {
                    if qa_md.items.iter().any(|item| item.name == name) {
                        skip(&mut qa_md, line_number, line, "同じ名前の項目が既にある");
                        current_item = None;
                    } else {
                        qa_md.items.push(QaMdItem {
                            line: line_number,
                            name,
                            expected,
                            checked,
                            automation: None,
                            automation_detail: None,
                            record: None,
                            verified_at: None,
                            image_url: None,
                            spec_ids: Vec::new(),
                        });
                        current_item = Some((qa_md.items.len() - 1, indent));
                    }
                }
                None => {
                    skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "チェック項目が `- [ ] **項目名**: 期待動作` の形でない",
                    );
                    current_item = None;
                }
            }
            continue;
        }

        if let Some((item_index, item_indent)) = current_item
            && indent > item_indent
            && let Some(content) = trimmed.strip_prefix("- ")
        {
            let content = content.trim();
            if let Some(value) = content
                .strip_prefix("自動化:")
                .or_else(|| content.strip_prefix("自動化："))
            {
                match parse_automation(value.trim()) {
                    Some((automation, detail)) => {
                        let item = &mut qa_md.items[item_index];
                        item.automation = Some(automation);
                        item.automation_detail = detail;
                    }
                    None => skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "自動化の値が auto(...) / manual(...) / todo でない",
                    ),
                }
            } else if let Some(record) = parse_record(content) {
                let item = &mut qa_md.items[item_index];
                if item.record.is_none() {
                    item.record = Some(record);
                    record_indent = Some(indent);
                } else {
                    skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "項目に既にスキップ / 失敗の記録がある",
                    );
                }
            } else {
                skip(
                    &mut qa_md,
                    line_number,
                    line,
                    "項目の入れ子で解釈できない行 (自動化 / ⏭️ スキップ / ❌ 失敗 のどれでもない)",
                );
            }
            continue;
        }

        match section {
            Section::Spec => {
                if !trimmed.starts_with('|') {
                    skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "仕様チェックリストの表の外の行",
                    );
                    continue;
                }
                let cells: Vec<&str> = trimmed
                    .trim_matches('|')
                    .split('|')
                    .map(str::trim)
                    .collect();
                if cells.len() < 3 {
                    skip(
                        &mut qa_md,
                        line_number,
                        line,
                        "仕様チェックリストの行の列が 3 つ未満",
                    );
                    continue;
                }
                let is_header = cells[0].eq_ignore_ascii_case("ID");
                let is_separator = cells
                    .iter()
                    .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':'));
                if is_header || is_separator {
                    continue;
                }
                let item_name = match cells[2].trim_matches('*').trim() {
                    "" | "—" | "-" | "–" => None,
                    name => Some(name.to_owned()),
                };
                qa_md.spec_rows.push(QaMdSpecRow {
                    id: cells[0].to_owned(),
                    expected: cells[1].to_owned(),
                    item_name,
                });
            }
            Section::Items => skip(&mut qa_md, line_number, line, "解釈できない行"),
            Section::None | Section::Prose => {}
        }
    }

    // 仕様チェックリストの対応項目を項目側に写す。項目名に一致しない行は読み飛ばした行として報告する
    let spec_rows = qa_md.spec_rows.clone();
    for row in spec_rows {
        let Some(item_name) = row.item_name else {
            continue;
        };
        match qa_md.items.iter_mut().find(|item| item.name == item_name) {
            Some(item) => {
                if !item.spec_ids.contains(&row.id) {
                    item.spec_ids.push(row.id);
                }
            }
            None => {
                // 行番号は表の行を覚えていないため、表の行の内容で特定できる形にする
                qa_md.skipped.push(SkippedLine {
                    file: file.to_owned(),
                    line: 0,
                    content: format!("| {} | {} | {} |", row.id, row.expected, item_name),
                    reason: "仕様チェックリストの対応項目に一致するチェック項目が無い".to_owned(),
                });
            }
        }
    }
    qa_md
}

/// `## ` の後ろが `N. 名前` (番号付きのセクション) か。
fn is_numbered_heading(heading: &str) -> bool {
    let digits = heading.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && heading[digits..].starts_with('.')
}

/// `- [ ] ...` / `- [x] ...` / `- [X] ...` なら (チェック済みか, チェックボックスの後ろ)。
fn parse_checkbox(trimmed: &str) -> Option<(bool, &str)> {
    let rest = trimmed.strip_prefix("- [")?;
    let (mark, rest) = rest.split_at_checked(1)?;
    let rest = rest.strip_prefix("] ")?;
    match mark {
        " " => Some((false, rest.trim())),
        "x" | "X" => Some((true, rest.trim())),
        _ => None,
    }
}

/// `**項目名**: 期待動作` を (項目名, 期待動作) に分ける。`**:` が無ければ `None`。
/// 項目名は castle の `sync_evidence.sh` の `item_key` と同じく、最初の `**:` の前まで。
fn parse_bold_name(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("**")?;
    let (name, expected) = rest
        .split_once("**:")
        .or_else(|| rest.split_once("**："))?;
    if name.is_empty() {
        return None;
    }
    Some((name.to_owned(), expected.trim().to_owned()))
}

/// `自動化:` の後ろ (`auto(...)` / `manual(...)` / `todo`) を (種別, 括弧の中) にする。括弧は全角・半角のどちらでも、無くてもよい。
fn parse_automation(value: &str) -> Option<(QaAutomation, Option<String>)> {
    if value == "todo" {
        return Some((QaAutomation::Todo, None));
    }
    let (automation, rest) = if let Some(rest) = value.strip_prefix("auto") {
        (QaAutomation::Auto, rest)
    } else if let Some(rest) = value.strip_prefix("manual") {
        (QaAutomation::Manual, rest)
    } else {
        return None;
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return Some((automation, None));
    }
    let detail = rest
        .strip_prefix(['(', '（'])
        .and_then(|rest| rest.strip_suffix([')', '）']))
        .map(str::trim)?;
    Some((
        automation,
        Some(detail.to_owned()).filter(|detail| !detail.is_empty()),
    ))
}

/// `⏭️ スキップ: 理由` / `❌ 失敗: 事象` の行なら、その記録。絵文字の有無だけで見分け (`⏭` は異体字セレクタの有無を問わない)、
/// `:` (全角でも) の後ろを本文にする。`:` が無ければ絵文字の後ろ全体を本文にする。
fn parse_record(content: &str) -> Option<QaMdRecord> {
    let (kind, marker) = if content.contains('⏭') {
        (QaMdRecordKind::Skip, '⏭')
    } else if content.contains('❌') {
        (QaMdRecordKind::Fail, '❌')
    } else {
        return None;
    };
    let after_marker = &content[content.find(marker)? + marker.len_utf8()..];
    let note = after_marker
        .split_once([':', '：'])
        .map_or(after_marker, |(_, note)| note)
        .trim()
        .trim_start_matches('\u{fe0f}')
        .trim();
    Some(QaMdRecord {
        kind,
        note: note.to_owned(),
    })
}

/// `**確認日: YYYY-MM-DD**` (太字の中でも外でもよい) から日付を取り出し、保存する形 (`named_json::normalize_date`) にする。
fn verified_date(line: &str) -> Option<String> {
    let after = &line[line.find("確認日")? + "確認日".len()..];
    let token = after
        .trim_start_matches(['*', ' ', ':', '：'])
        .split(|c: char| c == '*' || c.is_whitespace())
        .next()?;
    normalize_date(token)
}

/// `<img src="URL" ...>` の URL。
fn image_source(line: &str) -> Option<String> {
    let after = line.find("src=").map(|index| &line[index + "src=".len()..])?;
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let url = after[quote.len_utf8()..].split(quote).next()?;
    Some(url.to_owned()).filter(|url| !url.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_accepts_both_paren_styles() {
        assert_eq!(
            parse_automation("auto（e2e/flows/list.yaml）"),
            Some((QaAutomation::Auto, Some("e2e/flows/list.yaml".into())))
        );
        assert_eq!(
            parse_automation("manual(目視)"),
            Some((QaAutomation::Manual, Some("目視".into())))
        );
        assert_eq!(parse_automation("todo"), Some((QaAutomation::Todo, None)));
        assert_eq!(parse_automation("auto"), Some((QaAutomation::Auto, None)));
        assert_eq!(parse_automation("later"), None);
        assert_eq!(parse_automation("auto(開きっぱなし"), None);
    }

    #[test]
    fn record_reads_both_emoji_forms_and_colons() {
        assert_eq!(
            parse_record("⏭️ スキップ: 実機が要る"),
            Some(QaMdRecord {
                kind: QaMdRecordKind::Skip,
                note: "実機が要る".into()
            })
        );
        assert_eq!(
            parse_record("❌ 失敗：落ちる。再現手順: 開く"),
            Some(QaMdRecord {
                kind: QaMdRecordKind::Fail,
                note: "落ちる。再現手順: 開く".into()
            })
        );
        assert_eq!(
            parse_record("⏭ 理由だけ").map(|record| record.note),
            Some("理由だけ".into())
        );
        assert_eq!(parse_record("自動化: todo"), None);
    }

    #[test]
    fn verified_date_is_read_inside_or_outside_bold() {
        assert_eq!(
            verified_date("**確認日: 2026-10-11**").as_deref(),
            Some("2026-10-11")
        );
        assert_eq!(
            verified_date("**確認日:** 2026-1-2").as_deref(),
            Some("2026-01-02")
        );
        assert_eq!(verified_date("**確認日: 未定**"), None);
    }

    #[test]
    fn image_source_reads_the_url() {
        assert_eq!(
            image_source(r#"<img src="https://example.com/a.png" width="320">"#).as_deref(),
            Some("https://example.com/a.png")
        );
        assert_eq!(image_source("<img width=\"320\">"), None);
    }

    #[test]
    fn checkbox_and_bold_name() {
        assert_eq!(
            parse_checkbox("- [x] **一覧**: 出る"),
            Some((true, "**一覧**: 出る"))
        );
        assert_eq!(parse_checkbox("- [?] a"), None);
        assert_eq!(
            parse_bold_name("**一覧**: 出る"),
            Some(("一覧".into(), "出る".into()))
        );
        assert_eq!(parse_bold_name("一覧: 出る"), None);
        assert!(is_numbered_heading("1. 表示"));
        assert!(is_numbered_heading("12.編集"));
        assert!(!is_numbered_heading("横断確認項目"));
    }

    #[test]
    fn lines_outside_the_known_syntax_are_reported_not_fatal() {
        let qa_md = parse(
            "x/QA.md",
            "---\nfeature: f\ncolor: red\n---\n# f QA\n\n## 1. 表示\n\n- [ ] **一覧**: 出る\n  - 自動化: todo\n  - メモ\n- [ ] 名前なし\nただの文\n",
        );
        assert_eq!(qa_md.feature.as_deref(), Some("f"));
        assert_eq!(qa_md.items.len(), 1);
        let lines: Vec<(usize, &str)> = qa_md
            .skipped
            .iter()
            .map(|skipped| (skipped.line, skipped.content.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (3, "color: red"),
                (11, "  - メモ"),
                (12, "- [ ] 名前なし"),
                (13, "ただの文")
            ]
        );
    }
}
