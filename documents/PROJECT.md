# tansu

ローカルで動く Notion 風のデータベース / CMS。自由にプロパティを定義した表 (データベース) を複数持ち、Web 画面で閲覧・絞り込み・入力し、AI agent からは CLI と MCP で読み書きする。最初のユースケースは、castle の setup-qa / run-qa skill が 10 リポジトリ・158 ファイルの QA.md に持っている QA の項目と実行記録の置き換え。

なぜ作るか・何で成否を判定するか・MVP の機能の一覧は [DIRECTION.md](DIRECTION.md) にある。このファイルは、実装が守る要件と制約を持つ。起票元: https://github.com/bannzai/IdeaMemo/issues/350 (調査と決定の経緯は https://github.com/bannzai/castle/issues/1349 )。

## 構成

コア (スキーマ・問い合わせ・更新・QA.md のパーサ) は Rust の crate で、tansu の入口のプロセス (CLI・`tansu mcp`・`tansu serve`) が同じ crate を通して DB を開く (同時に開く前提の設定は `.claude/rules/sqlite-database.md`)。DB は libSQL (`libsql` crate) で、ローカルの SQLite ファイル (`Builder::new_local`) と Turso Cloud (`Builder::new_remote`) のどちらを開くかを起動時の設定で選ぶ (下の「DB の置き場所」)。Web 画面は TypeScript × React (Vite) で、将来はコアを Wasm に build してブラウザに組み込む (DIRECTION.md「決めたこと」2026-10-11)。Wasm に組み込むのは問い合わせの計算・整形・パースで、DB への接続はサーバーが持ち続ける (ブラウザに Turso のトークンを渡さないため)。`tansu-core` は今は libsql (C の FFI) と tokio に常に依存していて wasm32 ではビルドできないので、Wasm 化の時に `db` モジュールを feature で切るか crate を分ける (その issue で決める)。雛形は Cargo workspace の crate (core / cli / mcp / server) と web/ の Vite + React で、立ち上げ PR #1 の TypeScript の雛形から作り直した。サーバーは各 Mac の localhost で動かす (DIRECTION.md「決めたこと」2026-10-11) ので、サーバーの crate は Rust (axum) で、ログイン・ホスティングは持たない。論理的な型と SQLite への持ち方は `documents/data-model.md`。

| 入口 | 役割 |
| --- | --- |
| CLI `tansu` | すべての操作の入口。`--json` で機械可読の出力を返す。castle の skill とゲートはこの CLI を実行する |
| MCP サーバー (stdio) | CLI と同じ操作を tool として公開する。CLI = MCP の tool を引数で呼ぶ薄い入口で、両者は同じコアを共有する |
| Web 画面 | React の単一ページ。サーバーが配信し、同じサーバーの HTTP API を呼ぶ。Turso のトークンをブラウザに渡さない |

同じ処理を CLI・MCP・HTTP API の 3 箇所に書かない。コアは 1 つの crate に置き、3 つの入口はそれを呼ぶだけにする。

## 汎用のデータベース

- データベース = 自由に定義したプロパティを持つ表。プロパティの型は text / number / select / multi-select / checkbox / url / date / relation
- データベースは複数作れ、relation で表同士を結べる
- 各行は詳細ページを持つ: markdown の本文 + プロパティの値
- スキーマ (データベースとプロパティの定義) は SQLite の中に持ち、CLI の `db create` / `db schema` で変える。ファイルの手編集やマイグレーションのスクリプトを利用者に求めない。SQLite のスキーマの規約は `.claude/rules/sqlite-database.md`
- tansu がこのマシンに書くファイルはすべて `TANSU_HOME` (既定 `~/.tansu`。`~/.claude` / `~/.codex` と同じく、ホーム直下のツール名のドット付きディレクトリに置く方が、利用者がバックアップ・削除の対象を見つけやすいため) の下に置く。ただし DB だけは `TANSU_DB_URL` の `file:<パス>` で別の場所にもできる (テストは `TANSU_HOME` を一時ディレクトリにし、実際のホームディレクトリに書かない)

### DB の置き場所

環境変数 `TANSU_DB_URL` で選ぶ。未設定ならローカルのファイル。

| 値 | 開き方 | 複数の Mac |
| --- | --- | --- |
| 未設定、または `file:<パス>` (既定 `file:$TANSU_HOME/tansu.sqlite`。`file:` の後ろは URI ではなくファイルのパスそのもので、`//` やクエリは解釈しない。`file::memory:` (インメモリ) は非対応でエラーにする) | ローカルの SQLite ファイル | 同期しない (1 台で完結) |
| `libsql://<host>` + `TANSU_DB_AUTH_TOKEN` | Turso Cloud の DB に直接接続する (`Builder::new_remote`。マイグレーションの適用済みの判定は `schema_migration` テーブル。CI の `remote` job で確かめる。`.claude/rules/sqlite-database.md`) | 各 Mac が同じ DB を開くので同期の処理を持たない |

- Turso の embedded replica (ローカルに複製を持ち書き込みをクラウドへ送る) は使わない。ドキュメント ( https://docs.turso.tech/features/embedded-replicas/introduction ) は「Do not open the local database while the embedded replica is syncing. This can lead to data corruption.」と書いており、CLI・MCP・serve が別プロセスで同時に同じ DB を開く tansu の構成では同期中に開かないことを保証できないと判断した (別プロセスからの同時アクセスの可否はドキュメントに無く、ここは agent の推論)
- 同ページが新規の同期に勧める Turso Sync (ローカルで読み書きし `push()` / `pull()` で同期する) も第 1 弾では使わず、クラウド直結にする。必要になったら embedded replica と同じ観点 (複数プロセスの同時アクセス) で検討する
- Rust で書き直された Turso Database (旧 Limbo。改名の一次情報は未確認で、README には出てこない) は 1.0 前で複数プロセスの WAL が experimental のため ( https://github.com/tursodatabase/turso README、2026-10-11 時点)、現時点の本番の経路である libSQL を使う (同 README は将来 libSQL を置き換える方向と書いている)
- Turso のアカウント・DB・トークンの作成は利用者が行う (agent は外部サービスとの新規連携を行わない)

## CLI (汎用)

| コマンド | 役割 |
| --- | --- |
| `tansu db create <名前>` / `tansu db schema <名前> ...` | データベースとプロパティの定義 |
| `tansu query <データベース> --json [--where ...] [--sort ...]` | 絞り込み。出力は JSON |
| `tansu upsert <データベース> ...` / `tansu delete <データベース> <行 ID>` | 行の読み書き |
| `tansu export <データベース> --format markdown\|json` | 書き出し |
| `tansu serve [--host 127.0.0.1] [--port 7979] [--allow-host <ホスト名>]...` | Web 画面と HTTP API のサーバーを起動する。ポートの既定は 7979 (開発マシンで同時に動く他の dev サーバーの既定と重ならない番号。環境変数 `TANSU_PORT` でも指定でき、`--port` が優先)。`--allow-host` は「制約」の `Host` の許可リストに足す |
| `tansu mcp` | MCP サーバー (stdio) を起動する |

引数の形・JSON の形・exit code は、castle 側が依存する範囲を `documents/cli-contract.md` に書いて固定する。契約を変える時は同じ変更の中で更新する。

## QA 層 (汎用の上に載せる)

汎用部分 (スキーマ定義・表・画面・CLI・MCP) を QA 専用にしない。QA 固有の機能は汎用の DB の上に載せる層として `qa` サブコマンドに置く。

### データ

| データベース | 内容 |
| --- | --- |
| project | リポジトリ (`owner/repo`)、ローカルの checkout のパス |
| feature | project への relation、feature 名、QA.md の `verification` (動作確認手段)、QA.md があったディレクトリ `path` (project の checkout からの相対パス。`qa verified` が変更を見る範囲) |
| item | feature への relation、項目名、期待動作、自動化ステータス (`auto(<flow のパス>)` / `manual(<理由>)` / `todo`)、仕様 ID (S1, S2, ...) の紐付け、label (multi-select) |
| run | item への relation、結果 (OK / NG / あとで / スキップ)、メモ、記録日時 `recorded_at` (時差付きの ISO 8601。QA.md から取り込んだ記録は `**確認日:**` の日付)、記録の経路 `source` (select: `record` = `qa record` / Web 画面で記録、`import` = `qa import` で取り込み)、画像 URL、検証環境、検証した commit SHA (full) |

### CLI

| コマンド | 役割 |
| --- | --- |
| `tansu qa import <QA.md のパスまたはリポジトリのルート>... [--json] [--dry-run]` | QA.md 群を DB に取り込む (4 つの表が無ければ作る)。項目と自動化ステータスに加え、記録済みのエビデンス (`**確認日:**` と `<img src>`)・frontmatter の `last_verified_commit` / `last_verified_at`・⏭️ スキップ / ❌ 失敗の記録も run として取り込む。同じファイルを 2 回取り込んでも行が増えない (冪等)。`--json` は取り込んだ件数と読み飛ばした行を JSON で出し、`--dry-run` は書かずに同じ出力を出す。読み替えの規則は `documents/data-model.md`「QA 層」 |
| `tansu qa record (--item <ID> \| --repo <owner/repo> --feature <名前> --item-name <項目名>) --result ok\|ng\|later\|skip --commit <full SHA> [--note ...] [--image-url ...]... [--env ...] [--run-id <ID>] [--json]` | agent が実行結果を書く。実行のたびに run を 1 行足す (冪等性の扱いは「制約」) |
| `tansu qa verified --repo <owner/repo> --commit <full SHA> [--diff-base <commit>] --json` | その commit 以降に変更された feature と未検証の項目を返す。run の commit と `git diff` を突き合わせる。exit code は判定が通れば 0、再テストが要れば 1、エラーは 2。castle の release-app / auto-merge-pr のゲートがこれを呼ぶ |
| `tansu qa export --repo <owner/repo> --format pr-summary [--since <commit>]` | PR body に貼る「何をどう確認したか」の要約 |

### QA.md の形式

取り込む QA.md の形式は castle の `~/.claude/skills/setup-qa/references/qa-md-format.md` が SSOT (frontmatter の `feature` / `verification` / `last_verified_commit` / `last_verified_at`、`## N. セクション` の見出し、`- [ ] **項目名**: 期待動作`、入れ子の `自動化:`、`⏭️ スキップ:` / `❌ 失敗:`、`#### 動作確認` の details 内のエビデンスブロック)。パーサはこの文書の構文をすべて扱い、理解できない行で失敗せず、読み飛ばした行を報告する。

### QA.md の廃止後も残す性質

1. リポジトリの commit SHA を基準に「どの feature を再テストすべきか」を機械判定できる (`qa verified`)
2. PR 上で「何をどう確認したか」をレビューできる (`qa export --format pr-summary` の要約を PR body に貼る)

castle 側の置き換え (setup-qa / run-qa のフルスクラッチ #1357、release-app / auto-merge-pr のゲートと rules #1358、10 リポジトリの QA.md を import → 削除する skill #1359) は、QA 層の CLI の契約が main に入ってから castle の issue で進める。

## Web 画面

- データベースごとの表 / 一覧ビュー (Notion のテーブルビューに寄せる): 型ごとのセルとその場の編集、「+ 新規」の行、ツールバーでのプロパティ (label を含む) での絞り込みと並べ替え。行は表を残したまま右のパネル (サイドピーク) に開き、詳細ページ (全画面) でも編集できる
- QA の入力ビュー: 参考画像 ( https://x.com/yuto_apps/status/2105243014352478550 ) のように 1 項目ずつ OK / NG / あとで + メモを入力する。進捗フッター (入力済み / 全件、OK / NG / あとで の内訳)。「全部コピー」で未入力・NG を markdown にして agent に貼れる
- Mac のブラウザと、同じ LAN の iPhone から同じ画面を開ける (`tansu serve --host 0.0.0.0`)

## MCP サーバー

- stdio で動く。CLI と同じ操作を tool として公開する
- Claude Code (`.mcp.json`)、Codex CLI、pi の 3 つに同じサーバーを登録する手順を README に書く

## 複数の Mac の同期

「DB の置き場所」で Turso Cloud を選んだ時は各 Mac が同じ DB を開くので、同期の処理を持たない。ローカルのファイルを選んだ時は同期しない。サーバー (`tansu serve`) は各 Mac の `127.0.0.1` で動かす (DIRECTION.md「決めたこと」2026-10-11)。クラウドにサーバーを置く時はログイン・ホスティング・CD・法務ドキュメントが要件に加わるため、判定基準を通ってから検討する。

## 制約

- **既定は localhost だけ**: サーバーは既定で `127.0.0.1` で待ち受ける。ログインが無いため、LAN に開くのは `--host 0.0.0.0` を明示した時だけにする
- **`Host` の検査 (DNS rebinding 対策)**: HTTP API は `Host` が許可リストに無いリクエストを拒否する。許可リストは、`127.0.0.1` と `localhost` (ポート付き)、`--host` に指定した値、`--host 0.0.0.0` の時はこのマシンのネットワークインターフェースの IP (起動時に列挙する。iPhone はこれで接続する) に加え、`--allow-host <ホスト名>` で明示したもの (Tailscale の MagicDNS 名など)
- **書き込みの API は CSRF も防ぐ**: 書き込み (upsert / delete / `qa record` 等) は `Host` が正当でも別のサイトからの form POST や no-cors のリクエストで届くため、`Origin` が無いか `Host` と同じオリジンの時だけ受け付け、`Sec-Fetch-Site` が `cross-site` なら拒否し、本文は `Content-Type: application/json` だけを受け付ける (別のサイトがプリフライトなしで送れる形式を受け付けない)。CLI と MCP は HTTP を通らずコアを直接呼ぶため影響しない
- **外部への送信は DB の接続だけ**: 外部への計測・テレメトリを持たない。実行時の外部へのリクエストは、「DB の置き場所」で Turso Cloud を選んだ時のその DB への接続だけで、ローカルのファイルを選んだ時はマシンの外へ何も送らない。アプリが書くファイルは DB (SQLite の付随ファイル `-wal` / `-shm` を含む) と利用記録 `$TANSU_HOME/usage.jsonl` (画面を開いた日時と CLI・MCP の tool の実行の種類だけ。データの内容は書かない) だけで、利用記録は DIRECTION.md の判定基準の計測元になる
- **本物の QA.md をリポジトリに入れない**: private リポジトリの項目・エビデンスの URL を含むため。fixture とスクリーンショットは手書きの合成 QA.md から作る (`.claude/rules/synthetic-fixtures.md`)
- **冪等**: `qa import` と `upsert` は自然キー (取り込み元のファイルと項目名、行 ID) で upsert し、同じ入力で 2 回実行しても行が増えない。`qa record` は QA を実行した事実の記録なので、実行のたびに run を 1 行足す (同じ項目を 2 回確認すれば run は 2 行)。再試行で同じ実行が二重に入るのを防ぐ時は、呼び出し側が `--run-id <一意な ID>` を渡し、同じ `run-id` の run は 1 行にする

## インフラの決定

| 領域 | 決定 | 理由 |
| --- | --- | --- |
| DB・ストレージ | libSQL。利用者のマシンの SQLite 1 ファイル、または Turso Cloud の DB (起動時に選ぶ) | ローカルで完結する使い方と、複数の Mac で同じ DB を使う使い方の両方を、接続先の違いだけで実現できる |
| ホスティング | 持たない。各 Mac の localhost で `tansu serve` を動かす | クラウドに置く時はログイン・CD・法務ドキュメントが要るため、判定基準を通ってから検討する |
| 認証 | サーバーを各 Mac の localhost で動かす間は持たない。既定の `127.0.0.1` での待ち受けと `Host` / `Origin` の検査で守る | 自分のマシンで 1 人が使う。LAN に開くのは明示した時だけ |
| 計測 | 手元の利用記録と DB の行数 | 外部への計測・テレメトリを持たない制約に合う外部サービスが無い |
| 通知 | Slack の `#tansu-notification` (castle の slack-notification-setup skill で作成) | 関門の投稿先。サービスからの通知は持たない |
| アラート (GCP・Crashlytics)・課金・ストア配布・紹介サイト | 対象外 | クラウドのプロジェクト・モバイルアプリ・支払いが無い |
| 法務ドキュメント (利用規約・プライバシーポリシー) | サーバーを各 Mac の localhost で動かす間は対象外 (README の「データの扱い」で代える) | 利用者は bannzai 1 人で、外部への送信は利用者自身が選んだ Turso Cloud の DB への接続だけ (ローカルを選べば無い)。サーバーをクラウドに置いて他人も使うなら要る (「複数の Mac の同期」) |

## 検証

ビルド・テスト・画面の確認の方法は [AGENTS.md](../AGENTS.md) にある。要点: 開発マシンではビルドもブラウザでの表示も行わず、GitHub Actions が合成の fixture で行う。
