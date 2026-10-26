# データモデル

コア (Rust の crate) が扱う論理的な型と、その SQLite / libSQL への持ち方。`documents/PROJECT.md`「汎用のデータベース」「QA 層」の要件をこの型で固定する。JSON はこの型の serde の表現 (`snake_case` のキー、enum は `type` キーで判別) を基にし、CLI の行の入出力だけは ID を名前に置き換える (下の「CLI の入出力」)。castle 側が依存する範囲は `documents/cli-contract.md` に書く。決定の経緯は `documents/DIRECTION.md`「決めたこと」2026-10-11。

## 汎用層

型の定義は `crates/core/src/model.rs` にある (serde の表現もそこ)。ここには写しを置かない。

## SQLite / libSQL への持ち方

物理テーブルは固定の 3 つで、利用者の表はこの 3 つのデータとして持つ (利用者の操作で DDL を実行しない。`.claude/rules/sqlite-database.md`)。

| テーブル | 列 |
| --- | --- |
| `database` | `id`, `name` (unique), `created_at` |
| `property` | `id`, `database_id`, `name`, `kind` (JSON。`PropertyKind` の serde), `unique_value`, `position`。unique(`database_id`, `name`) |
| `row` | `id`, `database_id`, `body`, `values` (JSON。`BTreeMap<Id, Value>` の serde), `created_at`, `updated_at` |

このほかに、当て終えたマイグレーションの番号を持つ `schema_migration` (`version`) がある (利用者のデータは持たない。`.claude/rules/sqlite-database.md`)。

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
- `query --where` の演算子は `=` (等価。multi_select / relation はその選択肢・行 ID を含む行)、`^=` (前方一致。text / url / date)、`>=` / `<=` / `>` / `<` (比較。number / date)。演算子は `<` `>` `=` のうち最初に出た文字の位置から読み、`>=` / `<=` を `>` / `<` より先に見る (そのため `=` `<` `>` を含む名前と末尾が `^` の名前のプロパティは作れない)。date の比較の値は保存時と同じ書き方 (`2026-10-11` か RFC 3339) で、並べ替えと同じく UTC に直した時刻で比べる (日付だけの値はその日の 0 時 UTC)。値の無い行は比較に一致しない。ほかの型に比較を使うとエラーにする
- multi_select / relation の値は重複を除いて保存し、unique の判定では要素の順序を問わない。date の unique は保存した文字列で比べる (同じ時刻を別の時差で書いた値は別の値になる)
- `id` 無しの upsert で、複数の unique のプロパティの値が別々の行に一致した時は、最初に一致した行を書く対象にし、残りの行との重複として unique のエラーにする (どの行も書き換えない)
- `db create` / `db schema` / `db list` の出力は `Database` の serde (プロパティ・選択肢の ID を含む) のまま

## QA 層

`qa import` が最初に作る 4 つのデータベース (project / feature / item / run)。名前・プロパティ・unique の定義は `crates/core/src/qa/mod.rs` の `DATABASES` (汎用層の `db schema --add` と同じ指定) にあり、ここには写しを置かない。QA 層の行に専用の Rust の型は持たず、汎用層の `Row` をそのまま使う (QA 層の Rust の型は、`qa` サブコマンドの入力と JSON の出力の形だけ)。union のプロパティは汎用層に無いので、item の自動化ステータスは select `automation` (auto / manual / todo) + text `automation_detail` (auto の flow のパス、manual の理由) の 2 列に平たくする。run の `result` は select (ok / ng / later / skip)、`source` は select (record = `qa record` と Web 画面、import = `qa import`)、`recorded_at` は date (時差付き ISO 8601 か日付)、`commit` は text (full SHA)、`run_id` は text (unique。呼び出し側が渡す冪等キー)。

run は item の子で、`delete` で item を消すと `item` がその item だけの run も消える (`crates/core/src/qa/mod.rs` の `delete`。汎用層の delete は relation の値から ID を外すだけで、`item` の無い run が残るため)。

`qa import` (`crates/core/src/qa/import.rs`) の読み替え:

- project: `repo` (unique) は、取り込むルートが git リポジトリのルートそのものなら origin の URL の `owner/repo`、そうでなければディレクトリ名 (リポジトリの中の fixture のディレクトリを、囲むリポジトリの名前にしないため)。`path` はルートの絶対パス。QA.md のファイルを指定した時のルートは、それを含む git リポジトリのルート (git の外なら親ディレクトリ)
- 行を落とさない: QA.md の各行は、構造として読んだ行 (値に入れた行と、空行・`<details>`・`<summary>`・`---`・`（未実行）`・`#### 動作確認` のような書式だけの行。画像・`<details>`・`<summary>` の行にタグの外の文字 (setup-qa の書式が決める `動作確認スクショ` / `動作確認エビデンス` を除く) があれば、その文字を本文に残し、本文に残した行に数える) か、構造として読めず feature・item・run の本文に markdown のまま残した行 (`skipped` に報告する) のどちらか 1 つにする。パーサ (`crates/core/src/qa/qa_md.rs`) は行ごとに扱いを記録し、どちらにもならなかった行を `unaccounted` に出す。fenced code block は構文として読まず、開いた位置の本文 (エビデンスの中なら確認、項目の入れ子なら item、それ以外は feature) に残す
- feature: frontmatter の `feature` (無ければルート直下の QA.md は `_root`、それ以外は親ディレクトリ名)・`verification`・`last_verified_commit`・`last_verified_at` (取り込んだ時点の QA.md の値。run を作らない feature でも失われないように。`qa verified` は run から判定し、この 2 つは読まない)。`path` は QA.md があるディレクトリのルートからの相対パス (区切りは `/`。ルート直下は空文字)。project + 名前で行を決める。同じ project に同じ feature 名のファイルが 2 つ以上あれば、2 つ目以降はファイルごと取り込まず、全行を `unaccounted` に報告する (後のファイルが先のファイルの項目を黙って上書きしないため)
- feature の本文: 項目・エビデンスに当たらない行 (`# ` のタイトル、冒頭の段落、`## N.` 以外の `## ` のセクション (`## 関連リンク`・ルートのガイド・`## <変更> に関する注記` 等) を見出しごと、項目の間の地の文 (そのセクションの `## N.` 見出しを前に写す)、項目に一致しないエビデンスの見出しとその下、frontmatter の知らないキー) と、仕様チェックリストの表の写し (対応項目の無い仕様も残すため)。その後ろに `## QA.md の原文` の見出しと、QA.md の内容をそのまま入れた fenced code block を置く (上の規則で拾えない行があっても復元できる最後の受け皿。フェンスは原文のどのバッククォートの並びより長くする)。原文を専用のプロパティにしないのは、Web 画面の表のセルに全文が出て feature の表が読めなくなるため
- item: 項目名・期待動作・自動化 (`自動化:` の行が無い項目は値なし)・仕様チェックリストの対応 (`spec_ids`。multi_select の選択肢は取り込み時に足す。同じ名前の項目があれば最初の項目)・`section` (項目が属する `## ` 見出しの文字列。`1. 表示` 等)。`labels` (multi_select) は QA.md に無いので触らない。同じ feature に同じ名前の項目が 2 つ以上あっても別の item にし、名前は変えない。行は、feature の item のうち名前と期待動作が一致する行を先に当て、無ければ名前が一致する行 (追加した順) のうちまだ当てていないものに当て、それも無ければ足す (順番だけで当てると、前の項目を QA.md から消した時にその行と `qa record` の run が後ろの項目に移るため)。エビデンスの見出し (`### **項目名**: 期待動作`) も、名前と期待動作が一致する項目を先に当て (一部の項目にしかエビデンスが無い時に別の項目へ当てないため)、一致が無ければ同じ名前の項目に当てる (ファイルのどこかに名前と期待動作が一致する見出しがある項目は後回しにする)。どちらも、まだ見出しを当てていない最初の項目に当て、全部に当て終えていれば最後の項目に当てる。期待動作が当てた項目と違う見出しの行は、その項目の本文に残す
- item の本文: 入れ子の行のうち `自動化:` / ⏭️ / ❌ でないもの (補足・優先度等)、値が `auto(...)` / `manual(...)` / `todo` でない `自動化:` の行 (その値は `automation_detail` にも入れ、`automation` は値なし)、入れ子のコードブロック、エビデンスの見出しの下で最初の確認日より前の画像・説明文。記録の無い項目 (未チェックで ⏭️ / ❌ も無い) は run を作らないため、そのエビデンスの確認 (`**確認日: YYYY-MM-DD**` と確認の本文) も item の本文の後ろに足す。feature・item・run の本文は取り込みのたびに QA.md から作り直す (QA.md の現状と DB を合わせるため。Web 画面でそれらの本文に書いた内容は、同じ QA.md を取り込み直すと置き換わる)
- run: チェック済み (`ok`)・⏭️ スキップ (`skip`)・❌ 失敗 (`ng`) の項目に、エビデンスの確認ごとに 1 行 (確認が無ければ 1 行)。確認は `**確認日: ...**` / `**再確認日: ...**` の行 (行頭の `-` `*` の後ろが `確認日:` / `再確認日:` で始まる行。説明文の中の「確認日」は確認にしない) から、次の確認日かエビデンスの見出しまで。`source` は `import`、`run_id` は `import:<item の行 ID>:<確認の順番 (1 始まり)>`、`recorded_at` は確認日の先頭の日付 (`2026-09-05（注記）` の注記は run の本文。日付が読めなければその行を本文に残し、frontmatter の `last_verified_at`、それも無ければ値なし。取り込んだ時刻は書かない: 2 回目の取り込みで行が変わらないように)、`image_url` はその確認の最初の `<img src>`、本文はその確認の注記・説明文・すべての画像 (`![](<URL>)`)。`再確認日` の確認の本文は、補足語の「再」を落とさないよう先頭に `再確認` の行を置く。`result`・`note` (⏭️ / ❌ の本文とその下の併記事項。記号の後ろの `:` の前が `スキップ` / `失敗` でない時 (`⏭️ 一部スキップ: 理由` 等) は、その補足語を落とさないよう `一部スキップ: 理由` の形で残す。`result` の選択肢は増やさない)・`commit` (frontmatter の `last_verified_commit`) は、`qa verified` の「最後」と同じ順 (`recorded_at`、同じなら行を追加した順。これから足す行は既存の行の後ろに確認の順で並ぶ) で最後になる確認の run にだけ書き、それより前の確認の run は値なしにする (QA.md は項目の今の結果と最後に確認した commit しか持たないため)。既にある取り込みの run の行は、確認日と本文が同じ確認に当て、無ければ同じ `run_id` (同じ順番) の確認に当てる (行 ID と、取り込みが書かない `env` 等の値と、ほかの行からの relation を保つため。順番だけで当てると、前の確認を QA.md から消した時に後ろの確認の行が消える)。前の版の取り込みが作った `import:<item の行 ID>` の run は、日付が最後の確認 (同じ日付なら後の確認) に当たる行が無ければ、その確認の run に移す。その項目の run のうち `run_id` が `import:` で始まり、どの確認にも当たらなかったもの (記録・確認が消えたもの) は消す。`qa record` と Web 画面の run は消さない。`env` は触らない
- `--dry-run`: DB を開かない (開くとファイルの作成とマイグレーションで書き込みが起きる)。QA.md を読むだけで、取り込んだ時と同じ結果を返す。利用記録 (`usage.jsonl`) にはほかのコマンドと同じく 1 行足す

`qa record` (`crates/core/src/qa/record.rs`) が書く run: `source` は `record`、`recorded_at` は記録した時刻 (時差付き ISO 8601、秒の精度)、`run_id` は `--run-id` の値 (無ければ `record:<生成した ID>`)、`image_url` は最初の `--image-url`、本文はすべての `--image-url` を 1 行 1 つの markdown の画像にしたもの (`image_url` は url 型の 1 値で、`qa import` も確認の最初の画像を `image_url` に、すべての画像を本文に入れている)。`qa verified` / `qa export --format pr-summary` が読む規則と出力の形は `documents/cli-contract.md`。
