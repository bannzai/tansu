//! QA.md (castle の setup-qa / run-qa skill の形式。SSOT は castle の `setup-qa/references/qa-md-format.md`) のパーサ。
//! 1 行も落とさない: 各行を「構造として読んだ行」か「本文に残した行」([`QaMd::skipped`]。feature・項目・確認の本文に
//! markdown のまま入れる) のどちらかにし、どちらにもならなかった行を [`QaMd::unaccounted`] に出す
//! (`documents/data-model.md`「QA 層」)。理解できない行で失敗しない。

use std::collections::HashMap;

use serde::Serialize;

use super::markdown_link_destination;
use crate::named_json::normalize_date;

/// QA.md 1 ファイルを読んだ結果。frontmatter の 4 項目、チェック項目・仕様チェックリスト・feature の本文と、行の扱いの内訳を持つ。
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
    /// チェック項目。ファイルに出た順。同じ名前の項目も別の項目として持つ。
    pub items: Vec<QaMdItem>,
    /// 仕様チェックリストの表の行。対応項目は [`QaMdItem::spec_ids`] にも写してある。
    pub spec_rows: Vec<QaMdSpecRow>,
    /// feature の本文 (markdown)。項目・エビデンスに当たらない行 (タイトル・冒頭の段落・散文のセクション・項目の間の地の文・
    /// frontmatter の知らないキー) と、仕様チェックリストの表の写し。地の文の前にはそのセクションの `## ` 見出しを写す。
    pub body: String,
    /// 構造として読んだ行 (値に入れた行と、空行・`<details>`・`---` のような書式だけの行) の数。
    pub structured_lines: usize,
    /// 構造として読めず、本文 (feature・項目・確認のどれか) に残した行。
    pub skipped: Vec<SkippedLine>,
    /// 構造としても本文としても入らなかった行。パーサがすべての行を扱えていれば空。
    pub unaccounted: Vec<SkippedLine>,
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
    /// 項目が属する `## ` 見出しの文字列 (`1. 表示`)。見出しより前の項目は `None`。
    pub section: Option<String>,
    /// 入れ子の `自動化:` の種別。行が無い・値が形式外なら `None`。
    pub automation: Option<QaAutomation>,
    /// `自動化: auto(...)` / `manual(...)` の括弧の中 (flow のパス、自動化できない理由)。値が形式外の時はその値。
    pub automation_detail: Option<String>,
    /// 入れ子の `⏭️ スキップ:` / `❌ 失敗:` の記録。項目の入れ子に無ければエビデンスブロックの記録。
    pub record: Option<QaMdRecord>,
    /// エビデンスの確認 (`確認日` / `再確認日` ごと)。ファイルに出た順。
    pub confirmations: Vec<QaMdConfirmation>,
    /// 項目の本文 (markdown)。入れ子の補足の行・形式外の自動化の行・入れ子のコードブロックと、エビデンスの見出しの下で
    /// 最初の確認日より前にある画像・説明文。
    pub body: String,
    /// 仕様チェックリストでこの項目を対応項目にしている仕様 ID (S1, S2, ...)。表に出た順。
    pub spec_ids: Vec<String>,
}

/// エビデンスの確認 1 回分 (`**確認日: YYYY-MM-DD**` / `**再確認日: ...**` の行から、次の確認日かエビデンスの見出しの前まで)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QaMdConfirmation {
    /// 確認日の行の行番号 (1 始まり)。
    pub line: usize,
    /// 確認日。日付として読めなければ `None` (その行は [`QaMdConfirmation::body`] に残す)。
    pub date: Option<String>,
    /// この確認の最初の画像の URL。
    pub image_url: Option<String>,
    /// この確認の本文 (markdown)。日付の後ろの注記・説明文と、すべての画像 (`![](<URL>)`)。
    pub body: String,
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
    /// 表の行の行番号 (1 始まり)。
    pub line: usize,
    /// 仕様 ID (S1, S2, ...)。
    pub id: String,
    /// 期待挙動。
    pub expected: String,
    /// 対応項目の項目名。`—` (テスト漏れ) なら `None`。
    pub item_name: Option<String>,
}

/// 報告する行 ([`QaMd::skipped`] では本文に残した行、[`QaMd::unaccounted`] ではどこにも入らなかった行)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedLine {
    /// 行があるファイル (`parse` に渡した表示名)。
    pub file: String,
    /// 行番号 (1 始まり)。
    pub line: usize,
    /// 行の内容。
    pub content: String,
    /// 構造として読めなかった理由。
    pub reason: String,
}

/// 1 行を何として扱ったか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// 値に入れた行と、書式だけの行。
    Structured,
    /// 本文に残した行。
    Body,
}

/// 本文に残す行の行き先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyTarget {
    /// feature の本文。
    Feature,
    /// `items` の添字の項目の本文。
    Item(usize),
    /// (`items` の添字, その項目の `confirmations` の添字) の確認の本文。
    Confirmation(usize, usize),
}

/// `#### 動作確認` の details の中を読んでいる間の状態。
struct Evidence {
    /// `<details>` の深さ。0 に戻ったらブロックの終わり。
    depth: i32,
    /// 最初の `<details>` を見たか (見る前は深さ 0 でも終わりにしない)。
    started: bool,
    /// 直前の `### **項目名**` が指す項目の `items` の添字。項目に無い見出しの下は `None`。
    heading_item: Option<usize>,
    /// 直前の確認日の、`heading_item` の項目の `confirmations` の添字。見出しの後でまだ確認日が無ければ `None`。
    confirmation: Option<usize>,
}

impl Evidence {
    /// エビデンスの中の、構造として読めない行の行き先。
    fn target(&self) -> BodyTarget {
        match (self.heading_item, self.confirmation) {
            (Some(item_index), Some(confirmation_index)) => {
                BodyTarget::Confirmation(item_index, confirmation_index)
            }
            (Some(item_index), None) => BodyTarget::Item(item_index),
            (None, _) => BodyTarget::Feature,
        }
    }
}

/// `## ` 見出しで決まる、今いるセクションの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// 見出しの前。
    None,
    /// `## N. 名前` と `## 横断確認項目`。チェック項目を書く場所。
    Items,
    /// `## 仕様チェックリスト`。
    Spec,
    /// それ以外 (`## 関連リンク`、root QA.md の対象環境・起動方法・実行ナレッジ、`## <変更> に関する注記` 等)。見出しごと feature の本文に残す。
    Prose,
}

/// 読んでいる途中の結果と、行の扱いの記録。
struct Parser<'a> {
    /// `parse` に渡した表示名。
    file: &'a str,
    /// 組み立て中の結果。
    qa_md: QaMd,
    /// 行ごとの扱い (添字は行番号 - 1)。`None` の行はどこにも入っていない。
    kinds: Vec<Option<LineKind>>,
    /// 最後に本文を書いた行き先。空行で段落を区切る先。
    last_target: Option<BodyTarget>,
    /// 項目のセクションの `## ` 見出しで、まだ feature の本文に写していないもの。そのセクションの地の文を feature の本文に
    /// 書く時に先に写す (どのセクションの地の文かが本文から分かるように)。
    pending_heading: Option<String>,
    /// 今の項目のセクションが始まった時の `items` の数。セクションに項目が 1 つも無かったかを見分ける。
    section_first_item: usize,
}

impl Parser<'_> {
    /// 項目のセクションが項目も地の文も無いまま終わった時に、その `## ` 見出しを feature の本文に写す
    /// (見出しが項目の `section` にも本文にも入らず失われないように)。セクションの終わり (次の `## ` 見出しとファイルの終わり) で呼ぶ。
    fn flush_empty_section_heading(&mut self) {
        if self.qa_md.items.len() == self.section_first_item
            && let Some(heading) = self.pending_heading.take()
        {
            separate_paragraph(&mut self.qa_md.body);
            self.qa_md.body.push_str(&heading);
            self.qa_md.body.push('\n');
        }
        self.pending_heading = None;
    }

    /// `target` の本文。
    fn body_mut(&mut self, target: BodyTarget) -> &mut String {
        match target {
            BodyTarget::Feature => &mut self.qa_md.body,
            BodyTarget::Item(item_index) => &mut self.qa_md.items[item_index].body,
            BodyTarget::Confirmation(item_index, confirmation_index) => {
                &mut self.qa_md.items[item_index].confirmations[confirmation_index].body
            }
        }
    }

    /// 行 `line_number` を構造として読んだ行にする。
    fn structured(&mut self, line_number: usize) {
        self.kinds[line_number - 1] = Some(LineKind::Structured);
    }

    /// `target` の本文に `text` を 1 行足す。行の扱いは記録しない (構造として読んだ行の写しにも使うため)。
    fn copy(&mut self, target: BodyTarget, text: &str) {
        if target == BodyTarget::Feature
            && let Some(heading) = self.pending_heading.take()
        {
            separate_paragraph(&mut self.qa_md.body);
            self.qa_md.body.push_str(&heading);
            self.qa_md.body.push_str("\n\n");
        }
        let body = self.body_mut(target);
        body.push_str(text);
        body.push('\n');
        self.last_target = Some(target);
    }

    /// 行 `line_number` (内容 `content`) を、本文に残した行として記録する。本文には書かない ([`Parser::keep`] か [`Parser::copy`] が書く)。
    fn report_kept(&mut self, line_number: usize, content: &str, reason: &str) {
        self.kinds[line_number - 1] = Some(LineKind::Body);
        self.qa_md.skipped.push(SkippedLine {
            file: self.file.to_owned(),
            line: line_number,
            content: content.to_owned(),
            reason: reason.to_owned(),
        });
    }

    /// 行 `line_number` (内容 `content`) を `target` の本文に `text` として残し、本文に残した行として記録する。
    fn keep(
        &mut self,
        target: BodyTarget,
        line_number: usize,
        content: &str,
        text: &str,
        reason: &str,
    ) {
        self.copy(target, text);
        self.report_kept(line_number, content, reason);
    }

    /// 空行 `line_number`。構造として数え、最後に書いた本文の段落を区切る。
    fn blank(&mut self, line_number: usize) {
        self.structured(line_number);
        if let Some(target) = self.last_target {
            separate_paragraph(self.body_mut(target));
        }
    }
}

/// 本文 `body` が空でなく、空行で終わっていなければ空行を足す。
fn separate_paragraph(body: &mut String) {
    if !body.is_empty() && !body.ends_with("\n\n") {
        body.push('\n');
    }
}

/// `file` (表示名) の内容 `text` を読む。失敗せず、構造として読めない行は本文に残して [`QaMd::skipped`] に報告する。
///
/// 読む構文: frontmatter (`feature` / `verification` / `last_verified_commit` / `last_verified_at`)、`## N. セクション`、
/// `- [ ] **項目名**: 期待動作` と `- [x]`、入れ子の `自動化: auto(...) | manual(...) | todo` (括弧は全角・半角のどちらでも)、
/// `⏭️ スキップ:` / `❌ 失敗:` とその下の入れ子 (併記事項)、`## 仕様チェックリスト` の表、`#### 動作確認` の details の中の
/// `### **項目名**: 期待動作` と、`**確認日: YYYY-MM-DD**` / `**再確認日: YYYY-MM-DD**` (日付の後ろの注記は確認の本文) ごとの
/// `<img src="URL">` と説明文。同じ名前の項目は別の項目にし、同じ名前の k 番目のエビデンスの見出しを k 番目の項目に当てる。
pub fn parse(file: &str, text: &str) -> QaMd {
    let lines: Vec<&str> = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let mut parser = Parser {
        file,
        qa_md: QaMd::default(),
        kinds: vec![None; lines.len()],
        last_target: None,
        pending_heading: None,
        section_first_item: 0,
    };
    let mut section = Section::None;
    let mut section_name: Option<String> = None;
    let mut in_frontmatter = false;
    // 開いている fenced code block の (フェンスの文字, 長さ, 中身の行き先, 本文に書く時に外す行頭の空白の数)。
    // 閉じは CommonMark の規則 (同じ文字・開き以上の長さ・後ろは空白だけ)
    let mut code_fence: Option<(char, usize, BodyTarget, usize)> = None;
    let mut current_item: Option<(usize, usize)> = None;
    let mut record_indent: Option<usize> = None;
    let mut evidence: Option<Evidence> = None;
    let mut evidence_heading_counts: HashMap<String, usize> = HashMap::new();

    for (index, line) in lines.iter().copied().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if index == 0 && trimmed == "---" {
            in_frontmatter = true;
            parser.structured(line_number);
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
                parser.structured(line_number);
                continue;
            }
            if trimmed.is_empty() {
                parser.structured(line_number);
                continue;
            }
            let Some((key, value)) = trimmed.split_once(':') else {
                parser.keep(
                    BodyTarget::Feature,
                    line_number,
                    line,
                    line,
                    "frontmatter の行が `キー: 値` の形でない",
                );
                continue;
            };
            let value = match frontmatter_scalar(value) {
                "" | "null" => None,
                value => Some(value.to_owned()),
            };
            match key.trim() {
                "feature" => parser.qa_md.feature = value,
                "verification" => parser.qa_md.verification = value,
                "last_verified_commit" => parser.qa_md.last_verified_commit = value,
                // run の recorded_at (date) に写すため、日付として読めない値は本文に残す
                "last_verified_at" => match value.as_deref().map(normalize_date) {
                    Some(None) => {
                        parser.keep(
                            BodyTarget::Feature,
                            line_number,
                            line,
                            line,
                            "last_verified_at が YYYY-MM-DD で読めない",
                        );
                        continue;
                    }
                    date => parser.qa_md.last_verified_at = date.flatten(),
                },
                _ => {
                    parser.keep(
                        BodyTarget::Feature,
                        line_number,
                        line,
                        line,
                        "frontmatter の知らないキー",
                    );
                    continue;
                }
            }
            parser.structured(line_number);
            continue;
        }

        // fenced code block (記述例・コマンド) の中は構文として読まず、開いた位置の本文にそのまま残す
        if let Some((open_char, open_length, target, strip)) = code_fence {
            if let Some((close_char, close_length)) = code_fence_marker(trimmed)
                && close_char == open_char
                && close_length >= open_length
                && trimmed[close_length..].trim().is_empty()
            {
                code_fence = None;
            }
            parser.keep(
                target,
                line_number,
                line,
                dedent(line, strip),
                "コードブロックの中",
            );
            continue;
        }
        if let Some((fence_char, fence_length)) = code_fence_marker(trimmed) {
            let (target, strip) = match (&evidence, current_item) {
                (Some(state), _) => (state.target(), indent),
                (None, Some((item_index, item_indent))) if indent > item_indent => {
                    (BodyTarget::Item(item_index), item_indent + 2)
                }
                _ => (BodyTarget::Feature, 0),
            };
            code_fence = Some((fence_char, fence_length, target, strip));
            parser.keep(
                target,
                line_number,
                line,
                dedent(line, strip),
                "コードブロックの中",
            );
            continue;
        }

        // `<details>` が無い・閉じ忘れのエビデンスブロックは次の `## ` 見出しで終わりにする (以降を説明文にしないため)
        if evidence.is_some() && line.starts_with("## ") {
            evidence = None;
        }
        if let Some(state) = evidence.as_mut() {
            let opens = line.matches("<details").count();
            let closes = line.matches("</details>").count();
            if let Some(heading) = line.strip_prefix("### ") {
                state.confirmation = None;
                match parse_bold_name(heading.trim()) {
                    Some((name, _)) => {
                        let count = evidence_heading_counts.entry(name.clone()).or_default();
                        let occurrence = *count;
                        *count += 1;
                        let same_name: Vec<usize> = parser
                            .qa_md
                            .items
                            .iter()
                            .enumerate()
                            .filter(|(_, item)| item.name == name)
                            .map(|(item_index, _)| item_index)
                            .collect();
                        state.heading_item =
                            same_name.get(occurrence).or(same_name.last()).copied();
                        if state.heading_item.is_some() {
                            parser.structured(line_number);
                        } else {
                            parser.keep(
                                BodyTarget::Feature,
                                line_number,
                                line,
                                line,
                                "エビデンスの見出しの項目名に一致するチェック項目が無い",
                            );
                        }
                    }
                    None => {
                        state.heading_item = None;
                        parser.keep(
                            BodyTarget::Feature,
                            line_number,
                            line,
                            line,
                            "エビデンスの見出しが `### **項目名**: 期待動作` の形でない",
                        );
                    }
                }
            } else if let Some((date, annotation)) = confirmation_line(trimmed) {
                match state.heading_item {
                    Some(item_index) => {
                        let confirmations = &mut parser.qa_md.items[item_index].confirmations;
                        confirmations.push(QaMdConfirmation {
                            line: line_number,
                            date: date.clone(),
                            image_url: None,
                            body: String::new(),
                        });
                        let target = BodyTarget::Confirmation(item_index, confirmations.len() - 1);
                        state.confirmation = Some(confirmations.len() - 1);
                        if date.is_some() {
                            parser.structured(line_number);
                            parser.last_target = Some(target);
                            if !annotation.is_empty() {
                                parser.copy(target, &annotation);
                            }
                        } else {
                            parser.keep(
                                target,
                                line_number,
                                line,
                                trimmed,
                                "確認日の日付が YYYY-MM-DD で読めない",
                            );
                        }
                    }
                    None => parser.keep(
                        BodyTarget::Feature,
                        line_number,
                        line,
                        line,
                        "どの項目の確認日か分からない (エビデンスの見出しの下に無い)",
                    ),
                }
            } else if line.contains("<img") {
                let urls = image_sources(line);
                match state.target() {
                    BodyTarget::Feature => parser.keep(
                        BodyTarget::Feature,
                        line_number,
                        line,
                        line,
                        "どの項目の画像か分からない (エビデンスの見出しの下に無い)",
                    ),
                    target if urls.is_empty() => {
                        parser.keep(target, line_number, line, trimmed, "画像の src が無い")
                    }
                    target => {
                        if let BodyTarget::Confirmation(item_index, confirmation_index) = target {
                            let confirmation = &mut parser.qa_md.items[item_index].confirmations
                                [confirmation_index];
                            if confirmation.image_url.is_none() {
                                confirmation.image_url = urls.first().cloned();
                            }
                        }
                        for url in &urls {
                            parser
                                .copy(target, &format!("![]({})", markdown_link_destination(url)));
                        }
                        let text = text_outside_tags(line);
                        if text.is_empty() {
                            parser.structured(line_number);
                        } else {
                            parser.keep(target, line_number, line, &text, "画像の行の説明文");
                        }
                    }
                }
            } else if let Some(record) = parse_record(trimmed.trim_start_matches("- ").trim()) {
                match state.heading_item {
                    Some(item_index) if parser.qa_md.items[item_index].record.is_none() => {
                        parser.qa_md.items[item_index].record = Some(record);
                        parser.structured(line_number);
                    }
                    Some(_) => parser.keep(
                        state.target(),
                        line_number,
                        line,
                        trimmed,
                        "項目に既にスキップ / 失敗の記録がある",
                    ),
                    None => parser.keep(
                        BodyTarget::Feature,
                        line_number,
                        line,
                        line,
                        "どの項目の記録か分からない (エビデンスの見出しの下に無い)",
                    ),
                }
            } else if trimmed.is_empty() {
                parser.blank(line_number);
            } else if trimmed == "（未実行）" || trimmed == "(未実行)" {
                parser.structured(line_number);
            } else if trimmed.starts_with("<details")
                || trimmed.starts_with("</details>")
                || trimmed.starts_with("<summary")
            {
                // setup-qa の書式が決めている summary の文字列だけを書式として扱い、それ以外の文字は本文に残す
                let text = text_outside_tags(line);
                if ["", "動作確認スクショ", "動作確認エビデンス"].contains(&text.as_str()) {
                    parser.structured(line_number);
                } else {
                    parser.keep(
                        state.target(),
                        line_number,
                        line,
                        &text,
                        "details / summary の中の文字",
                    );
                }
            } else {
                // 入れ子のリストの字下げと、改行を表す行末の空白を本文の markdown に残すため、行をそのまま書く
                parser.keep(
                    state.target(),
                    line_number,
                    line,
                    line,
                    "エビデンスの説明文",
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
                confirmation: None,
            });
            current_item = None;
            record_indent = None;
            parser.structured(line_number);
            continue;
        }
        if let Some(heading) = line.strip_prefix("## ") {
            let heading = heading.trim();
            current_item = None;
            record_indent = None;
            parser.flush_empty_section_heading();
            section_name = Some(heading.to_owned());
            if heading == "仕様チェックリスト" {
                section = Section::Spec;
                separate_paragraph(&mut parser.qa_md.body);
                parser.copy(BodyTarget::Feature, line);
                parser.structured(line_number);
            } else if heading == "横断確認項目" || is_numbered_heading(heading) {
                section = Section::Items;
                parser.pending_heading = Some(line.to_owned());
                parser.section_first_item = parser.qa_md.items.len();
                parser.structured(line_number);
            } else {
                section = Section::Prose;
                separate_paragraph(&mut parser.qa_md.body);
                parser.keep(
                    BodyTarget::Feature,
                    line_number,
                    line,
                    line,
                    "散文のセクションの見出し",
                );
            }
            continue;
        }
        if trimmed.is_empty() {
            parser.blank(line_number);
            continue;
        }
        if trimmed == "---" {
            parser.structured(line_number);
            continue;
        }
        if line.starts_with("# ") {
            parser.keep(
                BodyTarget::Feature,
                line_number,
                line,
                line,
                "タイトルの見出し",
            );
            continue;
        }

        if let Some(record_start) = record_indent {
            if indent > record_start {
                if let Some(record) = current_item
                    .and_then(|(item_index, _)| parser.qa_md.items[item_index].record.as_mut())
                {
                    record.note.push('\n');
                    record
                        .note
                        .push_str(trimmed.strip_prefix("- ").unwrap_or(trimmed));
                    parser.structured(line_number);
                } else {
                    parser.keep(
                        BodyTarget::Feature,
                        line_number,
                        line,
                        line,
                        "記録の併記事項だが項目が無い",
                    );
                }
                continue;
            }
            record_indent = None;
        }

        if let Some((checked, rest)) = parse_checkbox(trimmed) {
            match parse_bold_name(rest) {
                Some((name, expected)) => {
                    parser.qa_md.items.push(QaMdItem {
                        line: line_number,
                        name,
                        expected,
                        checked,
                        section: section_name.clone(),
                        automation: None,
                        automation_detail: None,
                        record: None,
                        confirmations: Vec::new(),
                        body: String::new(),
                        spec_ids: Vec::new(),
                    });
                    current_item = Some((parser.qa_md.items.len() - 1, indent));
                    parser.structured(line_number);
                }
                None => {
                    parser.keep(
                        BodyTarget::Feature,
                        line_number,
                        line,
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
        {
            let target = BodyTarget::Item(item_index);
            let text = dedent(line, item_indent + 2);
            let content = trimmed.strip_prefix("- ").map(str::trim);
            let automation_value = content.and_then(|content| {
                content
                    .strip_prefix("自動化:")
                    .or_else(|| content.strip_prefix("自動化："))
                    .map(str::trim)
            });
            if let Some(value) = automation_value {
                let item = &mut parser.qa_md.items[item_index];
                match parse_automation(value) {
                    Some((automation, detail)) => {
                        item.automation = Some(automation);
                        item.automation_detail = detail;
                        parser.structured(line_number);
                    }
                    None => {
                        item.automation_detail = Some(value.to_owned());
                        parser.keep(
                            target,
                            line_number,
                            line,
                            text,
                            "自動化の値が auto(...) / manual(...) / todo でない",
                        );
                    }
                }
            } else if let Some(record) = content.and_then(parse_record) {
                let item = &mut parser.qa_md.items[item_index];
                if item.record.is_none() {
                    item.record = Some(record);
                    record_indent = Some(indent);
                    parser.structured(line_number);
                } else {
                    parser.keep(
                        target,
                        line_number,
                        line,
                        text,
                        "項目に既にスキップ / 失敗の記録がある",
                    );
                }
            } else {
                parser.keep(
                    target,
                    line_number,
                    line,
                    text,
                    "項目の補足 (自動化 / ⏭️ スキップ / ❌ 失敗 のどれでもない)",
                );
            }
            continue;
        }
        current_item = None;

        if section != Section::Spec {
            parser.keep(
                BodyTarget::Feature,
                line_number,
                line,
                line,
                "項目にもエビデンスにも当たらない地の文",
            );
            continue;
        }
        // 仕様チェックリストは表ごと feature の本文に写し (期待挙動と対応の無い仕様も残すため)、読めた行を構造として数える
        parser.copy(BodyTarget::Feature, line);
        if !trimmed.starts_with('|') {
            parser.report_kept(line_number, line, "仕様チェックリストの表の外の行");
            continue;
        }
        let cells = split_table_cells(trimmed);
        if cells.len() < 3 {
            parser.report_kept(line_number, line, "仕様チェックリストの行の列が 3 つ未満");
            continue;
        }
        let is_header = cells[0].eq_ignore_ascii_case("ID");
        let is_separator = cells
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':'));
        if is_header || is_separator {
            parser.structured(line_number);
            continue;
        }
        // 仕様 ID は multi_select の選択肢の名前になる。空と `,` 入りは選択肢にできない (`db schema --add` の形式)
        if cells[0].is_empty() || cells[0].contains(',') {
            parser.report_kept(
                line_number,
                line,
                "仕様 ID が空か `,` を含む (選択肢の名前にできない)",
            );
            continue;
        }
        let item_name = match cells[2].trim_matches('*').trim() {
            "" | "—" | "-" | "–" => None,
            name => Some(name.to_owned()),
        };
        parser.qa_md.spec_rows.push(QaMdSpecRow {
            line: line_number,
            id: cells[0].clone(),
            expected: cells[1].clone(),
            item_name,
        });
        parser.structured(line_number);
    }
    parser.flush_empty_section_heading();

    // 仕様チェックリストの対応項目を項目側 (同じ名前の項目があれば最初の項目) に写す。項目名に一致しない行は、
    // 表の写しとして feature の本文にあるので、本文に残した行として報告する
    let spec_rows = parser.qa_md.spec_rows.clone();
    for row in spec_rows {
        let Some(item_name) = row.item_name else {
            continue;
        };
        match parser
            .qa_md
            .items
            .iter_mut()
            .find(|item| item.name == item_name)
        {
            Some(item) => {
                if !item.spec_ids.contains(&row.id) {
                    item.spec_ids.push(row.id);
                }
            }
            None => parser.report_kept(
                row.line,
                lines[row.line - 1],
                "仕様チェックリストの対応項目に一致するチェック項目が無い",
            ),
        }
    }

    let Parser {
        file,
        mut qa_md,
        kinds,
        ..
    } = parser;
    qa_md.skipped.sort_by_key(|skipped| skipped.line);
    qa_md.structured_lines = kinds
        .iter()
        .filter(|kind| **kind == Some(LineKind::Structured))
        .count();
    qa_md.unaccounted = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| kind.is_none())
        .map(|(index, _)| SkippedLine {
            file: file.to_owned(),
            line: index + 1,
            content: lines[index].to_owned(),
            reason: "構造としても本文としても読まなかった".to_owned(),
        })
        .collect();
    trim_body(&mut qa_md.body);
    for item in &mut qa_md.items {
        trim_body(&mut item.body);
        for confirmation in &mut item.confirmations {
            trim_body(&mut confirmation.body);
        }
    }
    qa_md
}

/// 本文の末尾の改行を外す。
fn trim_body(body: &mut String) {
    body.truncate(body.trim_end_matches('\n').len());
}

/// `line` の行頭の空白を `width` 個まで外す (入れ子の行を、項目の本文の中で 1 段浅い markdown にするため)。
fn dedent(line: &str, width: usize) -> &str {
    let spaces = line.len() - line.trim_start_matches(' ').len();
    &line[spaces.min(width)..]
}

/// frontmatter の `キー:` の後ろの値。YAML の引用符 (`"..."` / `'...'`) で囲まれていればその中身 (エスケープは解釈しない。
/// setup-qa の scaffold は引用符無しで書くが、手で直した QA.md に引用符付きがある)。
fn frontmatter_scalar(value: &str) -> &str {
    let value = value.trim();
    ['"', '\'']
        .into_iter()
        .find_map(|quote| {
            value
                .strip_prefix(quote)
                .and_then(|inner| inner.strip_suffix(quote))
        })
        .unwrap_or(value)
}

/// Markdown の表の 1 行 (`| a | b |`) をセルに分ける。`\|` はセルの中の `|` として残す。
fn split_table_cells(row: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut characters = row.trim().trim_matches('|').chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' if characters.peek() == Some(&'|') => {
                characters.next();
                cells.last_mut().expect("1 つ目のセル").push('|');
            }
            '|' => cells.push(String::new()),
            _ => cells.last_mut().expect("1 つ目のセル").push(character),
        }
    }
    cells
        .into_iter()
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// 行頭が fenced code block のフェンス (` ``` ` か `~~~` を 3 つ以上) なら (フェンスの文字, 長さ)。
fn code_fence_marker(trimmed: &str) -> Option<(char, usize)> {
    let fence_char = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let length = trimmed.chars().take_while(|c| *c == fence_char).count();
    (length >= 3).then_some((fence_char, length))
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
    let (name, expected) = rest.split_once("**:").or_else(|| rest.split_once("**："))?;
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
    let (automation, rest) = match value.strip_prefix("auto") {
        Some(rest) => (QaAutomation::Auto, rest),
        None => (QaAutomation::Manual, value.strip_prefix("manual")?),
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
    // 行頭の絵文字だけで見分ける (本文に別の絵文字が出ても種別を変えない)
    let (kind, marker) = if content.starts_with('⏭') {
        (QaMdRecordKind::Skip, '⏭')
    } else if content.starts_with('❌') {
        (QaMdRecordKind::Fail, '❌')
    } else {
        return None;
    };
    let after_marker = &content[marker.len_utf8()..];
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

/// 行頭 (`-` `*` の後ろ) が `確認日:` / `再確認日:` (太字の中でも外でも、`:` は全角でも) の行なら、(日付, 日付の後ろの注記)。
/// 日付は先頭の数字と `-` の並びを保存する形 (`named_json::normalize_date`) にしたもので、読めなければ `None`。
/// 注記は日付の後ろの文字列から太字の `*` と前後の空白を外したもの (`（Android は未確認）` 等。無ければ空)。
fn confirmation_line(trimmed: &str) -> Option<(Option<String>, String)> {
    let label = trimmed.trim_start_matches(['-', '*', ' ']);
    let after_label = label
        .strip_prefix("再確認日")
        .or_else(|| label.strip_prefix("確認日"))?;
    let value = after_label
        .trim_start_matches(['*', ' '])
        .strip_prefix([':', '：'])?
        .trim_start_matches(['*', ' ']);
    let date_length = value
        .find(|c: char| !(c.is_ascii_digit() || c == '-'))
        .unwrap_or(value.len());
    Some((
        normalize_date(&value[..date_length]),
        value[date_length..]
            .trim()
            .trim_matches('*')
            .trim()
            .to_owned(),
    ))
}

/// 行から HTML のタグ (`<...>`) を除き、前後の空白を外した文字列 (画像・details の行に併記された説明文)。
fn text_outside_tags(line: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for character in line.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    text.trim().to_owned()
}

/// 行の中のすべての `<img src="URL" ...>` の URL (出た順)。
fn image_sources(line: &str) -> Vec<String> {
    line.match_indices("src=")
        .filter_map(|(index, _)| {
            let after = &line[index + "src=".len()..];
            let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let url = after[quote.len_utf8()..].split(quote).next()?;
            Some(url.to_owned()).filter(|url| !url.is_empty())
        })
        .collect()
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
        assert_eq!(parse_record("メモ: ❌ になる"), None);
        assert_eq!(
            parse_record("❌ 失敗: ⏭ ボタンが効かない").map(|record| record.kind),
            Some(QaMdRecordKind::Fail)
        );
    }

    #[test]
    fn confirmation_line_reads_the_leading_date_and_its_annotation() {
        assert_eq!(
            confirmation_line("**確認日: 2026-10-11**"),
            Some((Some("2026-10-11".into()), String::new()))
        );
        assert_eq!(
            confirmation_line("**確認日:** 2026-1-2"),
            Some((Some("2026-01-02".into()), String::new()))
        );
        assert_eq!(
            confirmation_line("**再確認日：2026-09-05（Android は未確認）**"),
            Some((Some("2026-09-05".into()), "（Android は未確認）".into()))
        );
        assert_eq!(
            confirmation_line("- 確認日: 2026-09-05"),
            Some((Some("2026-09-05".into()), String::new()))
        );
        assert_eq!(
            confirmation_line("**確認日: 未定**"),
            Some((None, "未定".into()))
        );
        // 説明文の中の「確認日」は確認の始まりにしない
        assert_eq!(confirmation_line("前回の確認日から画面が変わった"), None);
        assert_eq!(confirmation_line("確認日は未定"), None);
    }

    #[test]
    fn image_sources_reads_every_url_in_the_line() {
        assert_eq!(
            image_sources(r#"<img src="https://example.com/a.png" width="320">"#),
            ["https://example.com/a.png"]
        );
        assert_eq!(
            image_sources(r#"<img src="https://e.com/a.png"> <img src='https://e.com/b.png'>"#),
            ["https://e.com/a.png", "https://e.com/b.png"]
        );
        assert!(image_sources("<img width=\"320\">").is_empty());
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
    fn frontmatter_quotes_are_not_part_of_the_value() {
        let qa_md = parse(
            "x/QA.md",
            "---\nfeature: \"notes\"\nlast_verified_at: '2026-10-08'\nverification: 'a\"\n---\n",
        );
        assert_eq!(qa_md.feature.as_deref(), Some("notes"));
        assert_eq!(qa_md.last_verified_at.as_deref(), Some("2026-10-08"));
        assert_eq!(qa_md.verification.as_deref(), Some("'a\""));
    }

    #[test]
    fn escaped_pipes_stay_inside_table_cells() {
        assert_eq!(
            split_table_cells(r"| S1 | A \| B | 一覧 |"),
            ["S1", "A | B", "一覧"]
        );
        let qa_md = parse(
            "x/QA.md",
            "## 仕様チェックリスト\n\n| ID | 期待挙動 | 対応項目 |\n|---|---|---|\n| S1 | A \\| B | 一覧 |\n\n## 1. 表示\n\n- [ ] **一覧**: 出る\n",
        );
        assert_eq!(qa_md.spec_rows[0].expected, "A | B");
        assert_eq!(qa_md.items[0].spec_ids, ["S1"]);
        assert!(qa_md.skipped.is_empty(), "{:?}", qa_md.skipped);
        assert!(qa_md.unaccounted.is_empty(), "{:?}", qa_md.unaccounted);
        // 表は feature の本文に写す
        assert_eq!(
            qa_md.body,
            "## 仕様チェックリスト\n\n| ID | 期待挙動 | 対応項目 |\n|---|---|---|\n| S1 | A \\| B | 一覧 |"
        );
    }

    #[test]
    fn lines_outside_the_known_syntax_are_kept_in_a_body_and_reported() {
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
                (5, "# f QA"),
                (11, "  - メモ"),
                (12, "- [ ] 名前なし"),
                (13, "ただの文")
            ]
        );
        assert_eq!(qa_md.items[0].body, "- メモ");
        assert_eq!(qa_md.items[0].section.as_deref(), Some("1. 表示"));
        assert_eq!(
            qa_md.body,
            "color: red\n# f QA\n\n## 1. 表示\n\n- [ ] 名前なし\nただの文"
        );
        assert_eq!(qa_md.structured_lines + qa_md.skipped.len(), 13);
        assert!(qa_md.unaccounted.is_empty(), "{:?}", qa_md.unaccounted);
    }
}
