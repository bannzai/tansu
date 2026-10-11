//! CLI・MCP が入出力する行の JSON。DB には ID で持つ値 (プロパティ・select の選択肢) を、利用者が定義した名前で表す
//! (`documents/data-model.md`「CLI の入出力」)。
//!
//! ```json
//! {"id": "task-1", "database_id": "...", "body": "本文",
//!  "values": {"status": {"type": "select", "value": "done"}, "project": {"type": "relation", "value": ["p1"]}},
//!  "created_at": "...", "updated_at": "..."}
//! ```

use std::collections::BTreeMap;

use serde_json::json;

use crate::error::Error;
use crate::model::{Database, Id, Property, PropertyKind, Row, SelectOption, Value};

/// 行 `row` を、`values` のキーをプロパティ名、select / multi_select の値を選択肢の name にした JSON にする。
/// 値は [`Value`] の serde と同じく `type` と `value` を持つ。
pub fn row_to_json(database: &Database, row: &Row) -> serde_json::Value {
    let values: serde_json::Map<String, serde_json::Value> = database
        .properties
        .iter()
        .filter_map(|property| {
            row.values.get(&property.id).map(|value| {
                (
                    property.name.clone(),
                    json!({"type": property.kind.type_name(), "value": value_to_json(&property.kind, value)}),
                )
            })
        })
        .collect();
    json!({
        "id": row.id,
        "database_id": row.database_id,
        "body": row.body,
        "values": values,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

/// 1 つの値を、select / multi_select は選択肢の name にして JSON にする。
/// 選択肢が定義に無い ID (定義と値の食い違い) はそのまま ID を出す。
pub fn value_to_json(kind: &PropertyKind, value: &Value) -> serde_json::Value {
    let option_name = |options: &[SelectOption], id: &Id| {
        options
            .iter()
            .find(|option| option.id == *id)
            .map_or_else(|| id.0.clone(), |option| option.name.clone())
    };
    match (kind, value) {
        (PropertyKind::Select { options }, Value::Select(id)) => {
            json!(id.as_ref().map(|id| option_name(options, id)))
        }
        (PropertyKind::MultiSelect { options }, Value::MultiSelect(ids)) => {
            json!(
                ids.iter()
                    .map(|id| option_name(options, id))
                    .collect::<Vec<_>>()
            )
        }
        (_, Value::Text(text) | Value::Url(text) | Value::Date(text)) => json!(text),
        (_, Value::Number(number)) => json!(number),
        (_, Value::Checkbox(checked)) => json!(checked),
        (_, Value::Select(id)) => json!(id),
        (_, Value::MultiSelect(ids) | Value::Relation(ids)) => json!(ids),
    }
}

/// upsert に渡された行の JSON を、定義に照らして解釈した結果。
#[derive(Debug)]
pub struct RowInput {
    /// 書く行の ID。無ければ unique のプロパティの値で行を決める。
    pub id: Option<Id>,
    /// markdown 本文。無ければ既存の本文を変えない。
    pub body: Option<String>,
    /// 書く値。キーはプロパティの ID。`None` はそのプロパティの値を消す。
    pub values: BTreeMap<Id, Option<Value>>,
}

/// 行の JSON (`{"id"?, "body"?, "values"?: {<プロパティ名>: <値>}}`) を、データベース `database` の定義に照らして解釈する。
/// [`row_to_json`] の出力をそのまま渡せるよう、`database_id` / `created_at` / `updated_at` は読み飛ばす。
///
/// 値は `{"type": <型>, "value": <値>}` か、`value` の部分だけを書く。text / url / date は文字列 (date は `2026-10-11` か
/// RFC 3339)、number は数値、checkbox は真偽値、select は選択肢の name、multi_select は name の配列、relation は相手の行 ID の配列。
/// null は値を消す。
pub fn parse_row_input(database: &Database, input: &serde_json::Value) -> Result<RowInput, Error> {
    let object = input
        .as_object()
        .ok_or_else(|| Error::InvalidInput("行の JSON はオブジェクトで渡してください".into()))?;
    if let Some(key) = object.keys().find(|key| {
        !matches!(
            key.as_str(),
            "id" | "body" | "values" | "database_id" | "created_at" | "updated_at"
        )
    }) {
        return Err(Error::InvalidInput(format!(
            "行の JSON のキー `{key}` は解釈できません (id / body / values を使ってください)"
        )));
    }
    let id = match object.get("id") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(id)) if !id.is_empty() => Some(Id(id.clone())),
        Some(_) => {
            return Err(Error::InvalidInput(
                "行の JSON の id は空でない文字列にしてください".into(),
            ));
        }
    };
    let body = match object.get("body") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(body)) => Some(body.clone()),
        Some(_) => {
            return Err(Error::InvalidInput(
                "行の JSON の body は文字列にしてください".into(),
            ));
        }
    };
    let mut values = BTreeMap::new();
    match object.get("values") {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Object(named_values)) => {
            for (property_name, json) in named_values {
                let property =
                    database
                        .property(property_name)
                        .ok_or_else(|| Error::PropertyNotFound {
                            database: database.name.clone(),
                            property: property_name.clone(),
                        })?;
                values.insert(property.id.clone(), value_from_json(property, json)?);
            }
        }
        Some(_) => {
            return Err(Error::InvalidInput(
                "行の JSON の values は `{<プロパティ名>: <値>}` のオブジェクトにしてください"
                    .into(),
            ));
        }
    }
    Ok(RowInput { id, body, values })
}

/// プロパティ `property` の値として書かれた `json` を解釈する。null は `None` (値を消す)。書き方は [`parse_row_input`]。
fn value_from_json(property: &Property, json: &serde_json::Value) -> Result<Option<Value>, Error> {
    let json = match json {
        serde_json::Value::Object(tagged) => {
            if tagged.get("type").and_then(serde_json::Value::as_str)
                != Some(property.kind.type_name())
            {
                return Err(Error::InvalidInput(format!(
                    "プロパティ `{}` の値の type は `{}` にしてください",
                    property.name,
                    property.kind.type_name()
                )));
            }
            // `value` の書き間違いを、値を消す null と取り違えないようにエラーにする
            tagged.get("value").ok_or_else(|| {
                Error::InvalidInput(format!(
                    "プロパティ `{}` の値に `value` がありません (値を消す時は `\"value\": null` か null を書いてください)",
                    property.name
                ))
            })?
        }
        json => json,
    };
    // 空の配列 (multi_select / relation) は値が無いのと同じに扱い、値を消す。
    // 空の配列を値として残すと、unique の判定や行の特定で「空」同士が一致してしまう。relation の参照を外して空になった時も値を消している
    let empty_list = matches!(
        property.kind,
        PropertyKind::MultiSelect { .. } | PropertyKind::Relation { .. }
    ) && json.as_array().is_some_and(Vec::is_empty);
    if json.is_null() || empty_list {
        return Ok(None);
    }
    let invalid = |expected: &str| {
        Error::InvalidInput(format!(
            "プロパティ `{}` ({}) の値 `{json}` は{expected}ではありません",
            property.name,
            property.kind.type_name()
        ))
    };
    let string = || {
        json.as_str()
            .map(str::to_owned)
            .ok_or_else(|| invalid("文字列"))
    };
    let strings = || {
        json.as_array()
            .and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| invalid("文字列の配列"))
    };
    Ok(Some(match &property.kind {
        PropertyKind::Text => Value::Text(string()?),
        PropertyKind::Url => Value::Url(string()?),
        PropertyKind::Date => Value::Date(
            normalize_date(&string()?).ok_or_else(|| invalid("日付 (2026-10-11 か RFC 3339)"))?,
        ),
        PropertyKind::Number => Value::Number(json.as_f64().ok_or_else(|| invalid("数値"))?),
        PropertyKind::Checkbox => Value::Checkbox(json.as_bool().ok_or_else(|| invalid("真偽値"))?),
        PropertyKind::Select { options } => {
            Value::Select(Some(option_id(property, options, &string()?)?))
        }
        PropertyKind::MultiSelect { options } => Value::MultiSelect(without_duplicates(
            strings()?
                .iter()
                .map(|name| option_id(property, options, name))
                .collect::<Result<_, _>>()?,
        )),
        PropertyKind::Relation { .. } => {
            Value::Relation(without_duplicates(strings()?.into_iter().map(Id).collect()))
        }
    }))
}

/// `ids` から 2 回目以降に出た ID を除く (最初に出た順は保つ)。同じ選択肢・同じ行を 2 回持つ値に意味は無く、
/// 残すと unique の判定で `["p1", "p1"]` と `["p1"]` が別の値になるため。
fn without_duplicates(ids: Vec<Id>) -> Vec<Id> {
    let mut seen = std::collections::BTreeSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// date の値 `text` を保存する形に揃える。日付だけなら `2026-10-11`、日時なら RFC 3339 (区切りは大文字の `T`、UTC は `Z`)。
/// どちらとしても読めなければ `None`。
///
/// 入力の書き方 (`2026-1-2`、小文字の `t` / `z` 等) を揃えるのは、SQLite の日時関数 (並べ替えに使う `julianday`) が
/// ゼロ埋めした数字と大文字の区切りしか読まず、等価の絞り込みも保存した文字列と比べるため。
pub(crate) fn normalize_date(text: &str) -> Option<String> {
    if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(date.format("%Y-%m-%d").to_string());
    }
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|date_time| date_time.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
}

/// select / multi_select のプロパティ `property` の選択肢 `options` から、name が `name` の選択肢の ID を引く。
/// 無ければ [`Error::InvalidInput`] (選択肢は `db schema --add` で足す)。
pub(crate) fn option_id(
    property: &Property,
    options: &[SelectOption],
    name: &str,
) -> Result<Id, Error> {
    options
        .iter()
        .find(|option| option.name == name)
        .map(|option| option.id.clone())
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "プロパティ `{}` に選択肢 `{name}` はありません (選択肢: {})",
                property.name,
                options
                    .iter()
                    .map(|option| option.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Database {
        Database {
            id: Id("db".into()),
            name: "tasks".into(),
            properties: vec![
                Property {
                    id: Id("p_status".into()),
                    name: "status".into(),
                    kind: PropertyKind::Select {
                        options: vec![SelectOption {
                            id: Id("o_done".into()),
                            name: "done".into(),
                            color: None,
                        }],
                    },
                    unique: false,
                },
                Property {
                    id: Id("p_points".into()),
                    name: "points".into(),
                    kind: PropertyKind::Number,
                    unique: false,
                },
                Property {
                    id: Id("p_due".into()),
                    name: "due".into(),
                    kind: PropertyKind::Date,
                    unique: false,
                },
                Property {
                    id: Id("p_labels".into()),
                    name: "labels".into(),
                    kind: PropertyKind::MultiSelect {
                        options: vec![SelectOption {
                            id: Id("o_bug".into()),
                            name: "bug".into(),
                            color: None,
                        }],
                    },
                    unique: false,
                },
            ],
            created_at: "2026-10-11T00:00:00+09:00".into(),
        }
    }

    #[test]
    fn select_name_is_stored_as_option_id_and_shown_as_name() {
        let input = parse_row_input(
            &database(),
            &json!({"id": "r1", "values": {"status": "done", "points": {"type": "number", "value": 3}}}),
        )
        .unwrap();
        assert_eq!(input.id, Some(Id("r1".into())));
        assert_eq!(
            input.values,
            BTreeMap::from([
                (
                    Id("p_status".into()),
                    Some(Value::Select(Some(Id("o_done".into()))))
                ),
                (Id("p_points".into()), Some(Value::Number(3.0))),
            ])
        );
        let row = Row {
            id: Id("r1".into()),
            database_id: Id("db".into()),
            body: String::new(),
            values: input
                .values
                .into_iter()
                .filter_map(|(id, value)| value.map(|value| (id, value)))
                .collect(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert_eq!(
            row_to_json(&database(), &row)["values"],
            json!({"status": {"type": "select", "value": "done"}, "points": {"type": "number", "value": 3.0}})
        );
    }

    #[test]
    fn null_clears_the_value() {
        let input = parse_row_input(
            &database(),
            &json!({"id": "r1", "values": {"status": null}}),
        )
        .unwrap();
        assert_eq!(
            input.values,
            BTreeMap::from([(Id("p_status".into()), None)])
        );
    }

    #[test]
    fn empty_list_clears_multi_select_but_is_not_a_select_value() {
        let input =
            parse_row_input(&database(), &json!({"id": "r1", "values": {"labels": []}})).unwrap();
        assert_eq!(
            input.values,
            BTreeMap::from([(Id("p_labels".into()), None)])
        );
        assert!(
            parse_row_input(&database(), &json!({"id": "r1", "values": {"status": []}})).is_err()
        );
    }

    #[test]
    fn duplicated_items_are_stored_once() {
        let input = parse_row_input(
            &database(),
            &json!({"id": "r1", "values": {"labels": ["bug", "bug"]}}),
        )
        .unwrap();
        assert_eq!(
            input.values,
            BTreeMap::from([(
                Id("p_labels".into()),
                Some(Value::MultiSelect(vec![Id("o_bug".into())]))
            )])
        );
    }

    #[test]
    fn invalid_values_are_rejected() {
        for values in [
            json!({"status": "unknown"}),
            json!({"points": "3"}),
            json!({"points": {"type": "text", "value": 3}}),
            json!({"points": {"type": "number", "vale": 3}}),
            json!({"due": "10/11"}),
            json!({"missing": 1}),
        ] {
            assert!(
                parse_row_input(&database(), &json!({"id": "r1", "values": values})).is_err(),
                "{values} は受け付けない"
            );
        }
        assert!(parse_row_input(&database(), &json!({"id": "r1", "value": {}})).is_err());
        assert!(parse_row_input(&database(), &json!({"id": ""})).is_err());
    }

    #[test]
    fn date_accepts_date_only_and_rfc3339() {
        for due in ["2026-10-11", "2026-10-11T09:00:00+09:00"] {
            assert!(
                parse_row_input(&database(), &json!({"id": "r1", "values": {"due": due}})).is_ok()
            );
        }
    }

    #[test]
    fn date_is_normalized_to_the_form_sqlite_reads() {
        assert_eq!(normalize_date("2026-1-2").as_deref(), Some("2026-01-02"));
        assert_eq!(
            normalize_date("2026-10-11T09:00:00+09:00").as_deref(),
            Some("2026-10-11T09:00:00+09:00")
        );
        assert_eq!(
            normalize_date("2026-10-11t00:00:00.5+00:00").as_deref(),
            Some("2026-10-11T00:00:00.500Z")
        );
        assert_eq!(normalize_date("10/11"), None);
    }

    #[test]
    fn date_is_stored_with_uppercase_separators() {
        let input = parse_row_input(
            &database(),
            &json!({"id": "r1", "values": {"due": "2026-10-11t00:00:00z"}}),
        )
        .unwrap();
        assert_eq!(
            input.values[&Id("p_due".into())],
            Some(Value::Date("2026-10-11T00:00:00Z".into()))
        );
    }
}
