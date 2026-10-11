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
    let mut markdown = format!("# {}\n\n", database.name);
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
        let cells: Vec<String> = std::iter::once(escape_cell(&row.id.0))
            .chain(database.properties.iter().map(|property| {
                row.values
                    .get(&property.id)
                    .map(|value| escape_cell(&cell_text(&property.kind, value)))
                    .unwrap_or_default()
            }))
            .collect();
        markdown.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    for row in rows.iter().filter(|row| !row.body.is_empty()) {
        markdown.push_str(&format!("\n## {}\n\n{}\n", row.id.0, row.body.trim_end()));
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
fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', "<br>")
}
