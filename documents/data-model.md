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

`qa import` が最初に作る 4 つのデータベース (project / feature / item / run)。名前・プロパティ・unique の定義は `crates/core/src/qa/mod.rs` の `DATABASES` (汎用層の `db schema --add` と同じ指定) にあり、ここには写しを置かない。QA 層専用の Rust の型は持たず、汎用層の `Row` をそのまま使う (QA 層の CLI の issue で必要になった時に足す)。union のプロパティは汎用層に無いので、item の自動化ステータスは select `automation` (auto / manual / todo) + text `automation_detail` (auto の flow のパス、manual の理由) の 2 列に平たくする。run の `result` は select (ok / ng / later / skip)、`source` は select (record = `qa record` と Web 画面、import = `qa import`)、`recorded_at` は date (時差付き ISO 8601 か日付)、`commit` は text (full SHA)、`run_id` は text (unique。呼び出し側が渡す冪等キー)。

`qa import` (`crates/core/src/qa/import.rs`) の読み替え:

- project: `repo` (unique) は、取り込むルートが git リポジトリのルートそのものなら origin の URL の `owner/repo`、そうでなければディレクトリ名 (リポジトリの中の fixture のディレクトリを、囲むリポジトリの名前にしないため)。`path` はルートの絶対パス。QA.md のファイルを指定した時のルートは、それを含む git リポジトリのルート (git の外なら親ディレクトリ)
- feature: frontmatter の `feature` (無ければルート直下の QA.md は `_root`、それ以外は親ディレクトリ名) と `verification`。project + 名前で行を決める
- item: 項目名・期待動作・自動化 (`自動化:` の行が無い項目は値なし)・仕様チェックリストの対応 (`spec_ids`。multi_select の選択肢は取り込み時に足す)。feature + 名前で行を決め、同じファイルの同じ名前の 2 つ目以降は読み飛ばす。`labels` (multi_select) は QA.md に無いので触らない
- run: チェック済み (`ok`)・⏭️ スキップ (`skip`)・❌ 失敗 (`ng`) の項目に 1 行。`source` は `import`、`run_id` は `import:<item の行 ID>` (同じ項目の取り込みは 1 行にする)、`note` は ⏭️ / ❌ の本文とその下の併記事項、`recorded_at` はエビデンスの `**確認日: YYYY-MM-DD**` (無ければ frontmatter の `last_verified_at`、それも無ければ値なし。取り込んだ時刻は書かない: 2 回目の取り込みで行が変わらないように)、`image_url` はエビデンスの最初の `<img src>`、`commit` は frontmatter の `last_verified_commit`。`env` は触らない
