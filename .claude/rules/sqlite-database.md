---
paths:
  - "crates/**"
  - "**/*.sql"
---

# SQLite のデータの持ち方

tansu のデータは libSQL の DB 1 つ (既定はローカルの SQLite ファイル `$TANSU_HOME/tansu.sqlite`、`TANSU_HOME` の既定は `~/.tansu`。環境変数 `TANSU_DB_URL` で Turso Cloud の DB にも向けられる。選び方は `documents/PROJECT.md`「DB の置き場所」) にある。利用者のデータベースとプロパティの定義 (メタスキーマ) も同じ DB の中に持つ。以下の「ファイル」はローカルを選んだ時の SQLite ファイルを指す。Turso Cloud を選んだ時に当てはまるのは、物理スキーマの定義の置き場所・マイグレーション・メタスキーマ・トランザクションとプロセス内の排他の禁止・書き込みの冪等性・テストの一時ディレクトリの規約で、ファイルの開き方 (WAL・`busy_timeout`) だけがローカルだけの規約 (リモートでは接続先のサーバーが journal mode を持つと推定しているが未確認で、`PRAGMA journal_mode` を送ってよいかも含めて実装時に確かめる。`user_version` がリモートで使えるかも実装時に確かめ、使えなければマイグレーションの適用済みの判定をテーブルに持つ)。マイグレーションの競合と版の違うバイナリの扱いは実装した: ロック無しで `user_version` を読み、足りない時だけ `BEGIN IMMEDIATE` の中で読み直して当てる。DB の版がバイナリより新しければ `SchemaNewerThanBinary` で止める (どちらもローカルのファイルで検証済みで、リモートは未検証)。
- **物理スキーマの定義は 1 箇所**: SQLite のテーブル定義 (CREATE TABLE) は `crates/core/src/db/` のモジュールにだけ書く。コアの関数以外 (CLI・MCP・HTTP API・画面) は SQL を書かず、コアの関数を呼ぶ
- **マイグレーションは起動時に自動で当てる**: 物理スキーマの変更は、番号付きのマイグレーション (`crates/core/src/db/migrations/`) として足し、`user_version` で適用済みを判定して起動時に順に当てる。利用者にスクリプトの実行を求めない。当てたマイグレーションは書き換えず、変更は新しい番号で足す
- **利用者のデータベースの定義はデータとして持つ**: 利用者が `db create` / `db schema` で作る表とプロパティは、物理テーブルを増やさず、メタスキーマのテーブル (データベース・プロパティ・行。値は行の JSON 列) のデータとして持つ。利用者の操作で DDL を実行しない
- **DB を開くのは tansu のプロセスだけ**: SQLite ファイル (Turso Cloud の時はその DB) を開くのは tansu 自身の入口 (`tansu <CLI のサブコマンド>`・`tansu mcp`・`tansu serve`) で、どれも `crates/core/src/db/` の同じモジュールを通して開く。Web 画面 (ブラウザ)・castle の skill・ほかのツールは CLI か HTTP API を通して読み書きし、SQLite ファイルを直接開かない。Turso の CLI・ダッシュボードからクラウドの DB を直接書き換えない (スキーマの知識の分散を防ぐ)
- **複数のプロセスが同時に開く前提で設定する**: CLI・`tansu mcp`・`tansu serve` は別プロセスとして同時に動く (Turso Cloud では複数の Mac からも) ため、書き込みはトランザクションで囲み、排他が要る処理をプロセス内のメモリで管理しない。ローカルのファイルの時は、`crates/core/src/db/` が開くたびに WAL モード (`PRAGMA journal_mode=WAL`) と `busy_timeout` を設定する
- **書き込みは冪等にする**: 同じ入力で 2 回実行しても行が増えない (取り込みの元ファイルと項目名のような自然キーで upsert する)。実行の事実を記録する `qa record` の扱いは `documents/PROJECT.md`「制約」
- **テストは一時ディレクトリを使う**: `TANSU_HOME` をテストごとの一時ディレクトリにし (DB も利用記録もその下に入る)、実際のホームディレクトリに書かない
