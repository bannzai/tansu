# データモデル

コア (Rust の crate) が扱う論理的な型と、その SQLite / libSQL への持ち方。`documents/PROJECT.md`「汎用のデータベース」「QA 層」の要件をこの型で固定する。JSON はこの型の serde の表現 (`snake_case` のキー、enum は `type` キーで判別) を基にし、CLI の行の入出力だけは ID を名前に置き換える (下の「CLI の入出力」)。castle 側が依存する範囲は `documents/cli-contract.md` (QA 層の CLI の issue で作る) に書く。決定の経緯は `documents/DIRECTION.md`「決めたこと」2026-10-11。

## 汎用層

型の定義は `crates/core/src/model.rs` にある (serde の表現もそこ)。ここには写しを置かない。

## SQLite / libSQL への持ち方

物理テーブルは固定の 3 つで、利用者の表はこの 3 つのデータとして持つ (利用者の操作で DDL を実行しない。`.claude/rules/sqlite-database.md`)。

| テーブル | 列 |
| --- | --- |
| `database` | `id`, `name` (unique), `created_at` |
| `property` | `id`, `database_id`, `name`, `kind` (JSON。`PropertyKind` の serde), `unique_value`, `position`。unique(`database_id`, `name`) |
| `row` | `id`, `database_id`, `body`, `values` (JSON。`BTreeMap<Id, Value>` の serde), `created_at`, `updated_at` |

- 値は `row.values` の JSON 1 列に持つ (EAV の値テーブルにしない。この規模では JSON の方が実装と移行が単純で、絞り込みは `json_extract("values", '$.<property_id>')` で足りる。`values` は SQLite の予約語で引用符が必須)
- 絞り込み・並べ替えのインデックスは、必要になったプロパティに対して `CREATE INDEX ... ON "row"(json_extract("values", '$.<property_id>.<field>'))` の式インデックスを足す (最初は無し)
- `Property.unique` の検査はコアの upsert が行う (同じ `database_id` でその値を持つ行を検索してから書く。トランザクションの中で行う)
- 参照の整合 (`database_id` が実在する、`kind` / `values` が正しい JSON、`unique_value` が 0 か 1) は SQLite の FOREIGN KEY / CHECK ではなくコアの関数が担保する (DB を開くのは tansu のプロセスだけで、SQL を書くのはコアだけという規約 `.claude/rules/sqlite-database.md` に寄せる。制約を後から足すにはテーブルの再作成が要るため、ここで決めた)
- relation の相手の行を削除した時、参照している値はコアが消す (孤児を残さない)

## CLI の入出力

行の JSON は `crates/core/src/named_json.rs` が作り・読む。`Row` の serde との違いは、`values` のキーがプロパティ名で、select / multi_select の値が選択肢の name であること (保存は ID のまま)。relation の値は相手の行 ID。

```json
{"id": "task-1", "database_id": "...", "body": "本文",
 "values": {"status": {"type": "select", "value": "done"}, "project": {"type": "relation", "value": ["p1"]}},
 "created_at": "2026-10-11T09:00:00+09:00", "updated_at": "2026-10-11T09:00:00+09:00"}
```

- `upsert` の入力は同じ形で、`values` の値は `{"type", "value"}` のほか `value` の部分だけでも書ける。null と、multi_select / relation の空の配列はその値を消す。`database_id` / `created_at` / `updated_at` は読み飛ばす (query の出力をそのまま渡せる)
- `upsert` が書く行は、`id` があればその行 (無ければその `id` で足す)、`id` が無ければ unique のプロパティの値が一致する行 (無ければ新しい ID で足す)。どちらも無い入力はエラーにする (同じ入力を 2 回実行しても行が増えないように)
- date の値は `2026-10-11` か RFC 3339 の日時 (秒の小数はミリ秒まで) で、保存時に書き方を揃える (ゼロ埋め・大文字の `T`・UTC は `Z`)。並べ替えは時差を UTC に直した時刻で比べ、値の無い行は最後に置く
- multi_select / relation の値は重複を除いて保存し、unique の判定では要素の順序を問わない。date の unique は保存した文字列で比べる (同じ時刻を別の時差で書いた値は別の値になる)
- `id` 無しの upsert で、複数の unique のプロパティの値が別々の行に一致した時は、最初に一致した行を書く対象にし、残りの行との重複として unique のエラーにする (どの行も書き換えない)
- `db create` / `db schema` / `db list` の出力は `Database` の serde (プロパティ・選択肢の ID を含む) のまま

## QA 層

`qa import` が最初に作る 4 つのデータベース (Rust の型はまだ無い。QA 層の CLI の issue で、必要になった時に `crates/core` に足す)。汎用層の `Row` を次の形に読み替える (union のプロパティは汎用層に無いので、`automation` は select + text の 2 列に平たくする)。

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
