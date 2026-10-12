//! データベース 1 つ分の書き出し (`tansu export`)。

use serde_json::json;

use crate::model::{Database, PropertyKind, Row, Value};
use crate::named_json;

/// 定義と行を JSON にする。`database` は定義の serde (ID 付き)、`rows` は [`named_json::row_to_json`] の形。
pub fn to_json(database: &Database, rows: &[Row]) -> serde_json::Value {
    json!({
        "database": database,
        "rows": rows.iter().map(|row| named_json::row_to_json(database, row)).collect::<Vec<_>>(),
    })
}

/// 定義と行を markdown にする。見出しにデータベース名、行ごとに 1 行の表 (列は ID とプロパティ)、
/// 本文がある行はその後ろに行 ID の見出しと本文を続ける。select / multi_select は選択肢の name で書く。
pub fn to_markdown(database: &Database, rows: &[Row]) -> String {
    let mut markdown = format!("# {}\n\n", escape_cell(&database.name));
    let header: Vec<String> = std::iter::once("id".to_owned())
        .chain(
            database
                .properties
                .iter()
                .map(|property| escape_cell(&property.name)),
        )
        .collect();
    markdown.push_str(&format!("| {} |\n", header.join(" | ")));
    markdown.push_str(&format!("|{}\n", " --- |".repeat(header.len())));
    for row in rows {
        let cells: Vec<String> = std::iter::once(id_text(&row.id.0))
            .chain(database.properties.iter().map(|property| {
                row.values
                    .get(&property.id)
                    .map(|value| match value {
                        // relation の値は行 ID なので、id 列と同じく文字列をそのまま残す
                        Value::Relation(row_ids) => row_ids
                            .iter()
                            .map(|row_id| id_text(&row_id.0))
                            .collect::<Vec<_>>()
                            .join(", "),
                        _ => escape_cell(&cell_text(&property.kind, value)),
                    })
                    .unwrap_or_default()
            }))
            .collect();
        markdown.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    for row in rows.iter().filter(|row| !row.body.is_empty()) {
        markdown.push_str(&format!(
            "\n## {}\n\n{}\n",
            id_text(&row.id.0),
            row.body.trim_end()
        ));
    }
    markdown
}

/// 表のセルに書く値。配列は `, ` でつなぐ。
fn cell_text(kind: &PropertyKind, value: &Value) -> String {
    // JSON の数値は 3.0 のように小数点を残すため、表には Rust の表記 (3) で書く
    if let Value::Number(number) = value {
        return number.to_string();
    }
    match named_json::value_to_json(kind, value) {
        serde_json::Value::String(text) => text,
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map_or_else(|| item.to_string(), str::to_owned)
            })
            .collect::<Vec<_>>()
            .join(", "),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// 表の区切り `|` と改行をセルの中で使えるように置き換える。
/// 行 ID の書き方。agent が書き出しから ID を読んで `delete` / `upsert` に渡せるよう、コードスパンで文字列をそのまま残す
/// (`escape_cell` の `\_` のような記号のエスケープを付けない)。コードスパンで表せない ID (`` ` `` / `|` / 改行を含む) だけは
/// `escape_cell` にする。
fn id_text(id: &str) -> String {
    if id.contains(['`', '|', '\r', '\n']) {
        return escape_cell(id);
    }
    format!("`{id}`")
}

/// 保存した文字列をそのまま読めるよう、HTML の `<` `>` `&` を文字参照に、markdown の記号を `\` 付きにする。
/// 改行 (markdown は CR も改行として読むため CRLF・CR・LF のどれも) は、エスケープの後で `<br>` にする。
pub(crate) fn escape_cell(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                chars.next_if_eq(&'\n');
                escaped.push_str("<br>");
            }
            '\n' => escaped.push_str("<br>"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '\\' | '|' | '*' | '_' | '`' | '[' | ']' | '~' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_kept_as_written_in_a_code_span() {
        assert_eq!(id_text("ab_cd*1"), "`ab_cd*1`");
        assert_eq!(id_text("a|b"), "a\\|b");
    }

    #[test]
    fn cell_keeps_the_row_on_one_line() {
        assert_eq!(escape_cell("a|b\r\nc\rd\ne"), "a\\|b<br>c<br>d<br>e");
    }

    #[test]
    fn cell_shows_html_and_markdown_as_written() {
        assert_eq!(
            escape_cell(r"<input> & **重要** a_b `c` [d] ~e~ \"),
            r"&lt;input&gt; &amp; \*\*重要\*\* a\_b \`c\` \[d\] \~e\~ \\"
        );
    }
}
