---
paths:
  - "crates/**"
  - "**/*.sql"
---

# SQLite のデータの持ち方

tansu のデータは libSQL の DB 1 つ (既定はローカルの SQLite ファイル `$TANSU_HOME/tansu.sqlite`、`TANSU_HOME` の既定は `~/.tansu`。環境変数 `TANSU_DB_URL` で Turso Cloud の DB にも向けられる。選び方は `documents/PROJECT.md`「DB の置き場所」) にある。利用者のデータベースとプロパティの定義 (メタスキーマ) も同じ DB の中に持つ。以下の「ファイル」はローカルを選んだ時の SQLite ファイルを指す。Turso Cloud を選んだ時に当てはまるのは、物理スキーマの定義の置き場所・マイグレーション・メタスキーマ・トランザクションとプロセス内の排他の禁止・書き込みの冪等性・テストの一時ディレクトリの規約で、ファイルの開き方 (WAL・`busy_timeout`) だけがローカルだけの規約 (Turso Cloud は journal mode を変えさせず (https://docs.turso.tech/cloud/limitations「Turso Cloud manages journaling internally and does not allow changing the journal mode.」)、libsql のリモートの接続では `busy_timeout` が何もしない (libsql 0.9.30 `src/hrana/hyper.rs`) ため、リモートでは送らない)。

マイグレーションの適用済みの判定と、同時に開いた時・版の違うバイナリの扱い:

- **適用済みの判定は `schema_migration` テーブル**: 当て終えた番号を 1 行ずつ持つ (`crates/core/src/db/migrations/schema_migration.sql`)。`PRAGMA user_version` は Turso Cloud で読み取り専用のため使わない (https://docs.turso.tech/cloud/limitations「Read-only on Turso Cloud. Use a `_schema_version` table instead to track schema migrations.」)。ローカルとリモートで同じ経路にし、PR の CI のローカルのテストが同じコードを通るようにする。`schema_migration` が無いローカルの DB (`user_version` で記録していた版のバイナリが作った DB) は、初回に `user_version` の番号までを適用済みとして記録してから続きを当てる。リモートには `user_version` を送らない (これまで tansu はリモートを開けなかったため、移行する DB が無い)
- **同時に開いた時**: ロック無しで `schema_migration` を読み、足りない時だけ `BEGIN IMMEDIATE` の中で読み直して当てる。後から来た方はローカルでは `busy_timeout` の間待ち、リモートでは書き込みのロックを待つか busy で失敗する (失敗した時は再実行すれば、読み直しで当て終えた状態を見て何もしない。リモートで 2 台が同時に当てる経路は未検証)。`version` の PRIMARY KEY は、万一同じ番号を 2 回当てようとした時にトランザクションごと失敗させる最後の歯止めで、主の排他は `BEGIN IMMEDIATE`。ローカルのファイルを同一プロセスの別の接続から同時に開くテストで検証済み (別プロセスからの同時起動は未検証)
- **版の違うバイナリ**: DB の版 (`schema_migration` の最大の番号) がバイナリより新しければ `SchemaNewerThanBinary` で止める。`schema_migration` で記録する前の版のバイナリ (`user_version` を読む) が新しい DB を開くと、`user_version` が 0 のため 0001 を当て直そうとして「テーブルが既にある」で失敗し、コミットせずに止まるのでデータは壊れない (その時は tansu を更新する)

- **物理スキーマの定義は 1 箇所**: SQLite のテーブル定義 (CREATE TABLE) は `crates/core/src/db/` のモジュールにだけ書く。コアの関数以外 (CLI・MCP・HTTP API・画面) は SQL を書かず、コアの関数を呼ぶ
- **マイグレーションは起動時に自動で当てる**: 物理スキーマの変更は、番号付きのマイグレーション (`crates/core/src/db/migrations/`) として足し、`schema_migration` テーブルで適用済みを判定して起動時に順に当てる。利用者にスクリプトの実行を求めない。当てたマイグレーションは書き換えず、変更は新しい番号で足す
- **利用者のデータベースの定義はデータとして持つ**: 利用者が `db create` / `db schema` で作る表とプロパティは、物理テーブルを増やさず、メタスキーマのテーブル (データベース・プロパティ・行。値は行の JSON 列) のデータとして持つ。利用者の操作で DDL を実行しない
- **DB を開くのは tansu のプロセスだけ**: SQLite ファイル (Turso Cloud の時はその DB) を開くのは tansu 自身の入口 (`tansu <CLI のサブコマンド>`・`tansu mcp`・`tansu serve`) で、どれも `crates/core/src/db/` の同じモジュールを通して開く。Web 画面 (ブラウザ)・castle の skill・ほかのツールは CLI か HTTP API を通して読み書きし、SQLite ファイルを直接開かない。Turso の CLI・ダッシュボードからクラウドの DB を直接書き換えない (スキーマの知識の分散を防ぐ)
- **複数のプロセスが同時に開く前提で設定する**: CLI・`tansu mcp`・`tansu serve` は別プロセスとして同時に動く (Turso Cloud では複数の Mac からも) ため、書き込みはトランザクションで囲み、排他が要る処理をプロセス内のメモリで管理しない。ローカルのファイルの時は、`crates/core/src/db/` が開くたびに WAL モード (`PRAGMA journal_mode=WAL`) と `busy_timeout` を設定する。Turso Cloud の時は送らない。Turso Cloud の書き込みのトランザクションはサーバー側で 5 秒で打ち切られて巻き戻る (https://docs.turso.tech/sdk/ts/reference「Interactive transactions in libSQL lock the database for writing until committed or rolled back, with a 5-second timeout.」、libsql 0.9.30 `src/hrana/hyper.rs`「transaction will rollback automatically after timeout on the server side」) ため、1 つのトランザクションの中の問い合わせの往復を増やさない。いちばん重い `qa import` は CI の `remote` job が fixture で所要時間を計っている (`crates/core/tests/remote_db.rs`)。手元の Mac から Turso のリージョンまでの遅延が大きいと超え得て、その時は何も書かずに失敗する
- **書き込みは冪等にする**: 同じ入力で 2 回実行しても行が増えない (取り込みの元ファイルと項目名のような自然キーで upsert する)。実行の事実を記録する `qa record` の扱いは `documents/PROJECT.md`「制約」
- **テストは一時ディレクトリを使う**: `TANSU_HOME` をテストごとの一時ディレクトリにし (DB も利用記録もその下に入る)、実際のホームディレクトリに書かない
