//! 汎用層の論理的な型 (`documents/data-model.md`「汎用層」)。
//! CLI の `--json` と HTTP API の JSON はこの型の serde の表現で、キーは `snake_case`、enum は `type` キーで判別する。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 行・データベース・プロパティ・選択肢の ID。短い文字列 (nanoid 相当、21 文字) で、CLI の引数・URL・`--run-id` にそのまま書ける。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub String);

/// 利用者が定義する表 (Notion のデータベースに当たる)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Database {
    /// このデータベースの ID。
    pub id: Id,
    /// データベースの中で一意。
    pub name: String,
    /// `position` 順。
    pub properties: Vec<Property>,
    /// 時差付き ISO 8601。
    pub created_at: String,
}

/// 表の列の定義。型ごとに設定が違うので [`PropertyKind`] で持つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    /// このプロパティの ID。`Row.values` のキーになる。
    pub id: Id,
    /// 同じデータベースの中で一意。
    pub name: String,
    /// プロパティの型とその設定。
    pub kind: PropertyKind,
    /// true なら同じ値の行を 2 つ持てない (QA 層の `run_id`・`project.repo` が使う)。
    pub unique: bool,
}

/// プロパティの型とその設定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PropertyKind {
    /// 文字列。
    Text,
    /// 数値。
    Number,
    /// 真偽値。
    Checkbox,
    /// URL。
    Url,
    /// 日付または日時。
    Date,
    /// 選択肢から 1 つを選ぶ。
    Select {
        /// 選べる選択肢。
        options: Vec<SelectOption>,
    },
    /// 選択肢から複数を選ぶ。
    MultiSelect {
        /// 選べる選択肢。
        options: Vec<SelectOption>,
    },
    /// 別のデータベースの行への参照。片方向で、逆引きはクエリで出す。
    Relation {
        /// 参照先のデータベースの ID。
        target_database_id: Id,
    },
}

/// select / multi-select の選択肢。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectOption {
    /// この選択肢の ID。`Value` はこの ID で選択を保存する。
    pub id: Id,
    /// 利用者に見せる選択肢の名前。
    pub name: String,
    /// 表示の色。未指定なら画面の既定に任せる。
    pub color: Option<String>,
}

/// 1 つのセルの値。[`PropertyKind`] と 1 対 1 に対応し、値自身が型を持つ (`--json` の読み手が定義を引かずに解釈できるように)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    /// 文字列。
    Text(String),
    /// 数値。
    Number(f64),
    /// 真偽値。
    Checkbox(bool),
    /// URL。
    Url(String),
    /// ISO 8601。日付だけなら `"2026-10-11"`。
    Date(String),
    /// 選んだ選択肢の ID。未選択なら `None`。保存は ID で、CLI の入出力は name に変換する。
    Select(Option<Id>),
    /// 選んだ選択肢の ID の一覧。
    MultiSelect(Vec<Id>),
    /// 参照先の行の ID の一覧。
    Relation(Vec<Id>),
}

/// 表の 1 行 (Notion のページに当たる)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// この行の ID。
    pub id: Id,
    /// この行が属するデータベースの ID。
    pub database_id: Id,
    /// markdown 本文。
    pub body: String,
    /// キーはプロパティの ID。未設定のプロパティはキー無し。
    pub values: BTreeMap<Id, Value>,
    /// 時差付き ISO 8601。
    pub created_at: String,
    /// 時差付き ISO 8601。
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_is_tagged_by_type_with_its_content() {
        assert_eq!(
            serde_json::to_string(&Value::Text("a".into())).unwrap(),
            r#"{"type":"text","value":"a"}"#
        );
        assert_eq!(
            serde_json::to_string(&Value::MultiSelect(vec![Id("x".into())])).unwrap(),
            r#"{"type":"multi_select","value":["x"]}"#
        );
    }

    #[test]
    fn property_kind_is_tagged_by_type() {
        assert_eq!(
            serde_json::to_string(&PropertyKind::Relation {
                target_database_id: Id("db1".into())
            })
            .unwrap(),
            r#"{"type":"relation","target_database_id":"db1"}"#
        );
        assert_eq!(
            serde_json::to_string(&PropertyKind::Text).unwrap(),
            r#"{"type":"text"}"#
        );
    }

    #[test]
    fn row_values_are_keyed_by_property_id() {
        let row = Row {
            id: Id("r1".into()),
            database_id: Id("db1".into()),
            body: String::new(),
            values: BTreeMap::from([(Id("p1".into()), Value::Checkbox(true))]),
            created_at: "2026-10-11T00:00:00+09:00".into(),
            updated_at: "2026-10-11T00:00:00+09:00".into(),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert!(json.contains(r#""values":{"p1":{"type":"checkbox","value":true}}"#));
        assert_eq!(serde_json::from_str::<Row>(&json).unwrap(), row);
    }
}
