# データモデル

コア (Rust の crate) が扱う論理的な型と、その SQLite / libSQL への持ち方。`documents/PROJECT.md`「汎用のデータベース」「QA 層」の要件をこの型で固定する。CLI の `--json` と HTTP API の JSON はこの型の serde の表現 (`snake_case` のキー、enum は `type` キーで判別) で、castle 側が依存する範囲は `documents/cli-contract.md` (QA 層の CLI の issue で作る) に書く。決定の経緯は `documents/DIRECTION.md`「決めたこと」2026-10-11。

## 汎用層

```rust
/// 行・データベース・プロパティ・選択肢の ID。短い文字列 (nanoid 相当、21 文字) で、CLI の引数・URL・`--run-id` にそのまま書ける
pub struct Id(String);

/// 利用者が定義する表 (Notion のデータベースに当たる)
pub struct Database {
    pub id: Id,
    pub name: String,            // データベースの中で一意
    pub properties: Vec<Property>, // position 順
    pub created_at: String,      // 時差付き ISO 8601
}

/// 表の列の定義。型ごとに設定が違うので enum で持つ
pub struct Property {
    pub id: Id,
    pub name: String,            // 同じデータベースの中で一意
    pub kind: PropertyKind,
    pub unique: bool,            // true なら同じ値の行を 2 つ持てない (QA 層の run_id・project.repo が使う)
}

pub enum PropertyKind {
    Text,
    Number,
    Checkbox,
    Url,
    Date,
    Select { options: Vec<SelectOption> },
    MultiSelect { options: Vec<SelectOption> },
    Relation { target_database_id: Id }, // 片方向。逆引きはクエリで出す
}

pub struct SelectOption { pub id: Id, pub name: String, pub color: Option<String> }

/// 1 つのセルの値。PropertyKind と 1 対 1 に対応し、値自身が型を持つ (--json の読み手が定義を引かずに解釈できるように)
pub enum Value {
    Text(String),
    Number(f64),
    Checkbox(bool),
    Url(String),
    Date(String),                 // ISO 8601。日付だけなら "2026-10-11"
    Select(Option<Id>),           // 保存は選択肢の ID。CLI の入出力は name に変換する
    MultiSelect(Vec<Id>),
    Relation(Vec<Id>),            // 相手の行の ID
}

/// 表の 1 行 (Notion のページに当たる)
pub struct Row {
    pub id: Id,
    pub database_id: Id,
    pub body: String,                    // markdown 本文
    pub values: BTreeMap<Id, Value>,     // キーはプロパティの ID。未設定のプロパティはキー無し
    pub created_at: String,
    pub updated_at: String,
}
```

## SQLite / libSQL への持ち方

物理テーブルは固定の 3 つで、利用者の表はこの 3 つのデータとして持つ (利用者の操作で DDL を実行しない。`.claude/rules/sqlite-database.md`)。

| テーブル | 列 |
| --- | --- |
| `database` | `id`, `name` (unique), `created_at` |
| `property` | `id`, `database_id`, `name`, `kind` (JSON。`PropertyKind` の serde), `unique_value`, `position`。unique(`database_id`, `name`) |
| `row` | `id`, `database_id`, `body`, `values` (JSON。`BTreeMap<Id, Value>` の serde), `created_at`, `updated_at` |

- 値は `row.values` の JSON 1 列に持つ (EAV の値テーブルにしない。この規模では JSON の方が実装と移行が単純で、絞り込みは `json_extract(values, '$.<property_id>')` で足りる)
- 絞り込み・並べ替えのインデックスは、必要になったプロパティに対して `CREATE INDEX ... ON row(json_extract(values, '$.<property_id>.<field>'))` の式インデックスを足す (最初は無し)
- `Property.unique` の検査はコアの upsert が行う (同じ `database_id` でその値を持つ行を検索してから書く。トランザクションの中で行う)
- relation の相手の行を削除した時、参照している値はコアが消す (孤児を残さない)

## QA 層

`qa import` が最初に作る 4 つのデータベース。汎用層の `Row` を次の形に読み替える (union のプロパティは汎用層に無いので、`automation` は select + text の 2 列に平たくする)。

```rust
pub struct QaProject { pub repo: String /* unique */, pub path: Option<String> }

pub struct QaFeature { pub project: Id, pub name: String, pub verification: String }

pub struct QaItem {
    pub feature: Id,
    pub name: String,
    pub expected: String,
    pub automation: QaAutomation,        // select: auto / manual / todo
    pub automation_detail: Option<String>, // auto の flow のパス、manual の理由
    pub spec_ids: Vec<String>,           // multi_select。S1, S2, ...
    pub labels: Vec<String>,             // multi_select
}

pub enum QaAutomation { Auto, Manual, Todo }

pub struct QaRun {
    pub item: Id,
    pub result: QaResult,                // select: ok / ng / later / skip
    pub note: Option<String>,
    pub recorded_at: String,             // 時差付き ISO 8601。import は **確認日:** の日付
    pub source: QaSource,                // select: record / import
    pub image_url: Option<String>,
    pub env: Option<String>,
    pub commit: Option<String>,          // full SHA
    pub run_id: Option<String>,          // unique。呼び出し側が渡す冪等キー
}

pub enum QaResult { Ok, Ng, Later, Skip }
pub enum QaSource { Record, Import }
```
