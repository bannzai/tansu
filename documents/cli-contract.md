# CLI の契約

castle の skill (setup-qa / run-qa) とゲート (release-app / auto-merge-pr) が依存する `tansu` の CLI の引数・JSON の形・exit code。castle 側はこの文書を読んで実装する。ここに書いた形を変える時は、同じ変更の中でこの文書を更新する (`documents/PROJECT.md`「CLI (汎用)」)。実装は `crates/cli/src/main.rs` (引数) と `crates/core/src/qa/` (処理と JSON の型)、この文書とテストの JSON のキーの一致は `crates/cli/tests/cli.rs` の `cli_contract_lists_every_json_key_the_qa_commands_print` が確かめる。

## 全体

| 場面 | exit code | 出力 |
| --- | --- | --- |
| 成功 | 0 | 結果を stdout に出す (JSON は整形した 1 つの値) |
| 失敗 (下の `qa verified` を除く) | 1 | stdout は空。stderr に `tansu: <理由>` の 1 行 |
| 引数の誤り (必須の引数が無い・未知のサブコマンド・選べない値)、引数なし | 2 | clap のエラーかヘルプを stderr に出す |
| `--version` / `--help` | 0 | stdout に出す |

- DB と利用記録は `TANSU_HOME` (既定 `~/.tansu`) の下に置く。DB だけは `TANSU_DB_URL` で別の場所にもできる (`documents/PROJECT.md`「DB の置き場所」)
- 引数を解釈できた実行は、そのたびに利用記録 `$TANSU_HOME/usage.jsonl` に `{"event": <種類>, "at": <時差付き ISO 8601>}` を 1 行足す。`event` はサブコマンド名を `_` でつないだもの (`qa_record` / `qa_verified` / `qa_export` / `qa_import` / `query` / `db_create` 等)。データの内容は書かない。書けなかった時は stderr に警告を 1 行出し、exit code は変えない
- commit を受け取る引数のうち `--commit` は full SHA (40 桁か 64 桁の 16 進) だけを受け付ける (短縮 SHA や `HEAD` は記録した後に別の commit を指し得るため)。`--diff-base` / `--since` は project の checkout で解決できる任意の commit の指定 (ref・短縮 SHA を含む) を受け付ける

## 行の JSON

`qa record --json` が出す run の行は、汎用の `query` / `upsert` と同じ形 (`documents/data-model.md`「CLI の入出力」)。

| キー | 型 | 内容 |
| --- | --- | --- |
| `id` | string | 行 ID |
| `database_id` | string | データベースの ID |
| `body` | string | markdown の本文 |
| `values` | object | プロパティ名 → `{"type": string, "value": <型ごとの値>}`。値の無いプロパティはキーが無い |
| `created_at` / `updated_at` | string | 時差付き ISO 8601 |

run の `values` のプロパティ (`type` と `value` の型):

| キー | `type` | `value` |
| --- | --- | --- |
| `item` | `relation` | string の配列 (item の行 ID が 1 つ) |
| `result` | `select` | `ok` / `ng` / `later` / `skip` |
| `note` | `text` | string |
| `recorded_at` | `date` | string (`qa record` は時差付き ISO 8601。`qa import` は `YYYY-MM-DD`) |
| `source` | `select` | `record` (`qa record`) / `import` (`qa import`) |
| `image_url` | `url` | string |
| `env` | `text` | string |
| `commit` | `text` | string (full SHA) |
| `run_id` | `text` | string |

## `tansu qa import <パス>... [--json] [--dry-run]`

QA.md 群を QA 層の 4 つのデータベース (project / feature / item / run) に取り込む。パスはリポジトリのルート (配下の `QA.md` をすべて探す) か QA.md のファイル。同じ入力を 2 回取り込んでも行は増えない。読み替えの規則は `documents/data-model.md`「QA 層」。

- `--dry-run`: DB を開かず何も書かない (利用記録は足す)。出力は取り込んだ時と同じ
- 出力 (`--json`): 下の object。`--json` 無しは件数の 1 行と、読み飛ばした行を 1 行ずつ
- exit code: 成功 0、失敗 1 (読めないパス等。その時は何も書かない)

| キー | 型 | 内容 |
| --- | --- | --- |
| `dry_run` | bool | `--dry-run` なら true |
| `files` | string の配列 | 読んだ QA.md の絶対パス |
| `counts` | object | 入力に含まれる行の数 (新しく足した数ではない)。`project` / `feature` / `item` / `run` (number) |
| `skipped` | object の配列 | 解釈できずに読み飛ばした行。`file` (string)、`line` (number。1 始まり)、`content` (string。行の内容)、`reason` (string) |

## `tansu qa record`

```sh
tansu qa record (--item <item の行 ID> | --repo <owner/repo> --feature <feature 名> --item-name <項目名>) \
  --result ok|ng|later|skip --commit <full SHA> \
  [--note <メモ>] [--image-url <URL>]... [--env <検証環境>] [--run-id <一意な ID>] [--json]
```

QA を実行した結果を run に 1 行足す。

- 実行のたびに 1 行足す (同じ項目を 2 回確認すれば 2 行)。`--run-id` を渡した時は、同じ `run_id` の run を 1 行にし、2 回目以降はその行を今回の値で置き換える (渡さなかった値は消える)。再試行で同じ実行を二重に記録しないために使う。`--run-id` 無しの `run_id` は `record:<生成した ID>`
- `--result` は大文字でもよい (`OK` / `NG`)。`--commit` は省略できない (exit 2)
- `--image-url` は複数指定できる。最初の URL を `image_url` に、すべての URL を本文 (`body`) に 1 行 1 つの markdown の画像 (`![](<URL>)`。URL の中の `<` `>` は `%3C` `%3E` にする) で書く。空の値と、空白・改行・制御文字を含む値は受け付けない (本文と `qa export --format pr-summary` の markdown を崩すため)
- `source` は `record`、`recorded_at` は実行した時刻 (時差付き ISO 8601、秒の精度)
- QA 層の 4 つのデータベースが無ければ、記録と同じトランザクションで作る (失敗した時は何も作らない。`qa import` の前は項目が無いので失敗する)
- 出力: `--json` なら書いた run の行の JSON (上の「行の JSON」)。無ければ `run <行 ID> を記録しました` の 1 行
- exit code: 成功 0、失敗 1 (項目が無い・`--commit` が full SHA でない・`--result` が選択肢に無い・`--run-id` が空か `import:` で始まる・`--image-url` が上の条件に合わない)、引数の誤り 2

## `tansu qa verified --repo <owner/repo> --commit <full SHA> [--diff-base <commit>] --json`

commit を基準に、再テストが要る feature と未検証の item を返す。DB には書かない。git を project の checkout (`qa import` が project の `path` に書いた絶対パス) で実行する。

判定の規則 (feature ごと):

- `last_verified_commit`: その feature の item の run のうち、`result` が `ok` か `skip` で `commit` を持つ最後の run の `commit`。「最後」は `recorded_at` が後のもの、同じ値 (秒の精度) なら後に追加されたもの。`recorded_at` はまず書かれた時差での日付で比べ、同じ日付なら日付だけの値 (`qa import` の run) を時刻付きの値 (`qa record` の run) より前にし、時刻付きどうしは UTC の時刻で比べる。`recorded_at` の無い run はどの値よりも前とみなす
- `changed_files`: `git diff --name-only --relative --no-renames <last_verified_commit> <commit> -- <feature のディレクトリ>` のファイルから `QA.md` を除いたもの。パスは project の checkout からの相対パスで、日本語などを含むパスもエスケープせずにそのまま出す。リネームは削除と追加に分けるので、ほかのディレクトリへ移したファイルは移動元と移動先の両方の feature に入る
  - feature のディレクトリは、QA.md があったディレクトリ (feature の `path`)
  - `last_verified_commit` が無い (null)、full SHA でない (`qa import` が取り込んだ短縮 SHA 等。大文字の full SHA は使う)、またはリポジトリで解決できない (rebase で消えた・合成の SHA 等) feature は、確かめた時点が分からないので `<commit>` の時点のディレクトリの全ファイル (`QA.md` を除く) を入れる
  - ルート直下の QA.md の feature (`path` が空) と、item の無い feature は diff せず空にする
- `unverified_items`: run が 1 つも無い item。NG / あとで の run だけがある item は含めない (未解消の NG は呼び出し側が `query run --json --where result=ng` 等で見る)
- `--diff-base` を渡すと、`git diff --name-only --relative --no-renames <diff-base> <commit>` (2 点の比較) で feature のディレクトリの下に変更 (`QA.md` を除く) がある feature だけを判定の対象にする。対象外の feature は `features` に出さず、`ok` にも数えない。ルート直下の feature は対象にならない。PR のゲートでは `git merge-base <base ブランチ> <head>` の commit を渡す (PR が触っていない feature の古い未検証で PR を止めないため)

出力 (`--json` は必須):

| キー | 型 | 内容 |
| --- | --- | --- |
| `repo` | string | 指定した `owner/repo` |
| `commit` | string | 指定した commit (小文字の full SHA) |
| `ok` | bool | 対象のすべての feature で `changed_files` と `unverified_items` が空なら true |
| `features` | object の配列 | 対象の feature (取り込んだ順)。要素のキーは下の 4 つ |
| `feature` | string | feature 名 |
| `last_verified_commit` | string か null | 上の規則 |
| `changed_files` | string の配列 | 上の規則 |
| `unverified_items` | object の配列 | `id` (string。item の行 ID。`qa record --item` に渡せる)、`name` (string。項目名) |

exit code: `ok` が true なら 0、false なら 1、エラー (project が無い・`--commit` が full SHA でないかリポジトリに無い・`--diff-base` を解決できない・git の失敗・`--json` が無い・feature に `path` が無い (`path` を持たない版の `qa import` で取り込んだ。取り込み直す)) は 2。エラーの時は stdout は空で、stderr に 1 行を出す。castle のゲートは exit code で判定する。

## `tansu qa export --repo <owner/repo> --format pr-summary [--since <commit>]`

PR body に貼る「何をどう確認したか」の markdown を stdout に出す。DB には書かない。表の markdown / JSON の書き出しは汎用の `tansu export run --format markdown|json` を使う (`--format` は `pr-summary` だけを受け付ける)。

- 見出し `## QA の記録 (<owner/repo>)` の後に、feature ごと (取り込んだ順) に `### <feature 名>` と表を出す。見出しとセルの文字列は markdown の記号をエスケープする (`note_list` は `note\_list`。汎用の `export --format markdown` と同じ規則)。表の列は `| 項目 | 結果 | 確認日 | 画像 | commit |`
- 行は item ごと (取り込んだ順) に、その item の最後の run (「最後」は `qa verified` と同じ) を 1 行。結果は `OK` / `NG` / `あとで` / `スキップ`、確認日は `recorded_at` の先頭の `YYYY-MM-DD`、画像は `image_url` へのリンク `[画像](<URL>)` (本文の 2 つ目以降の画像は出さない)、commit は 7 桁の短縮 SHA (16 進でない値はエスケープしてそのまま)。URL の中の `|` `\` `<` `>` 改行はパーセントエンコードする。値の無いセルは `—`
- run の無い item と、行の無い feature は出さない。表が 1 つも無い時は `記録された run はありません` の 1 行
- `--since` を渡すと、`commit` が `<since>` の子孫 (`git merge-base --is-ancestor <since> <commit>`。`<since>` 自身は含めない。run の `commit` は短縮 SHA でも full SHA に解決してから比べる) の run だけを見る。`commit` の無い run と、リポジトリで解決できない run は見ない。PR の base を渡すと、その PR で記録した run だけになる
- exit code: 成功 0、失敗 1 (project が無い・`--since` を解決できない・`--since` を渡したのに project に checkout のパスが無い)、引数の誤り 2
