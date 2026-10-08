# tansu

ローカルで動く Notion 風のデータベース / CMS。自由にプロパティを定義した表 (データベース) を複数持ち、Web 画面で閲覧・絞り込み・入力し、AI agent からは CLI と MCP で読み書きする。最初のユースケースは、castle の setup-qa / run-qa skill が 10 リポジトリ・158 ファイルの QA.md に持っている QA の項目と実行記録の置き換え。

なぜ作るか・何で成否を判定するか・MVP の機能の一覧は [DIRECTION.md](DIRECTION.md) にある。このファイルは、実装が守る要件と制約を持つ。起票元: https://github.com/bannzai/IdeaMemo/issues/350 (調査と決定の経緯は https://github.com/bannzai/castle/issues/1349 )。

## 構成

サーバー 1 プロセス + SQLite 1 ファイル。実行環境は Bun (TypeScript)。SQLite は Bun 組み込みの `bun:sqlite` を使い、ネイティブ拡張の依存を持たない。

| 入口 | 役割 |
| --- | --- |
| CLI `tansu` | すべての操作の入口。`--json` で機械可読の出力を返す。castle の skill とゲートはこの CLI を実行する |
| MCP サーバー (stdio) | CLI と同じ操作を tool として公開する。CLI = MCP の tool を引数で呼ぶ薄い入口で、両者は同じコアを共有する |
| Web 画面 | 単一ページの HTML。サーバーが配信し、同じサーバーの HTTP API を呼ぶ |

同じ処理を CLI・MCP・HTTP API の 3 箇所に書かない。コア (スキーマ・問い合わせ・更新) は 1 つのモジュール群に置き、3 つの入口はそれを呼ぶだけにする。

## 汎用のデータベース

- データベース = 自由に定義したプロパティを持つ表。プロパティの型は text / number / select / multi-select / checkbox / url / date / relation
- データベースは複数作れ、relation で表同士を結べる
- 各行は詳細ページを持つ: markdown の本文 + プロパティの値
- スキーマ (データベースとプロパティの定義) は SQLite の中に持ち、CLI の `db create` / `db schema` で変える。ファイルの手編集やマイグレーションのスクリプトを利用者に求めない。SQLite のスキーマの規約は `.claude/rules/sqlite-database.md`
- tansu がこのマシンに書くファイルはすべて `TANSU_HOME` (既定 `~/.tansu`。`~/.claude` / `~/.codex` と同じく、ホーム直下のツール名のドット付きディレクトリに置く方が、利用者がバックアップ・削除の対象を見つけやすいため) の下に置く。DB ファイルの既定は `$TANSU_HOME/tansu.sqlite` で、環境変数 `TANSU_DB` があればそのパスを優先する (テストは `TANSU_HOME` を一時ディレクトリにし、実際のホームディレクトリに書かない)

## CLI (汎用)

| コマンド | 役割 |
| --- | --- |
| `tansu db create <名前>` / `tansu db schema <名前> ...` | データベースとプロパティの定義 |
| `tansu query <データベース> --json [--where ...] [--sort ...]` | 絞り込み。出力は JSON |
| `tansu upsert <データベース> ...` / `tansu delete <データベース> <行 ID>` | 行の読み書き |
| `tansu export <データベース> --format markdown\|json` | 書き出し |
| `tansu serve [--host 127.0.0.1] [--port <番号>]` | Web 画面と HTTP API のサーバーを起動する |
| `tansu mcp` | MCP サーバー (stdio) を起動する |

引数の形・JSON の形・exit code は、castle 側が依存する範囲を `documents/cli-contract.md` に書いて固定する (QA 層の CLI の issue で作る)。契約を変える時は同じ変更の中で更新する。

## QA 層 (汎用の上に載せる)

汎用部分 (スキーマ定義・表・画面・CLI・MCP) を QA 専用にしない。QA 固有の機能は汎用の DB の上に載せる層として `qa` サブコマンドに置く。

### データ

| データベース | 内容 |
| --- | --- |
| project | リポジトリ (`owner/repo`)、ローカルの checkout のパス |
| feature | project への relation、feature 名、QA.md の `verification` (動作確認手段) |
| item | feature への relation、項目名、期待動作、自動化ステータス (`auto(<flow のパス>)` / `manual(<理由>)` / `todo`)、仕様 ID (S1, S2, ...) の紐付け、label (multi-select) |
| run | item への relation、結果 (OK / NG / あとで / スキップ)、メモ、記録日時 `recorded_at` (時差付きの ISO 8601。QA.md から取り込んだ記録は `**確認日:**` の日付)、画像 URL、検証環境、検証した commit SHA (full) |

### CLI

| コマンド | 役割 |
| --- | --- |
| `tansu qa import <QA.md のパスまたはリポジトリのルート>...` | QA.md 群を DB に取り込む。項目と自動化ステータスに加え、記録済みのエビデンス (`**確認日:**` と `<img src>`)・frontmatter の `last_verified_commit` / `last_verified_at`・⏭️ スキップ / ❌ 失敗の記録も run として取り込む。同じファイルを 2 回取り込んでも行が増えない (冪等) |
| `tansu qa record --item <ID> --result OK\|NG\|later\|skip --commit <sha> [--note ...] [--image-url ...] [--env ...] [--run-id <ID>]` | agent が実行結果を書く。実行のたびに run を 1 行足す (冪等性の扱いは「制約」) |
| `tansu qa verified --repo <owner/repo> --commit <sha> --json` | その commit 以降に変更された feature と未検証の項目を返す。run の commit と `git diff` を突き合わせる。castle の release-app / auto-merge-pr のゲートがこれを呼ぶ |
| `tansu qa export --repo <owner/repo> --format pr-summary` | PR body に貼る「何をどう確認したか」の要約 |

### QA.md の形式

取り込む QA.md の形式は castle の `~/.claude/skills/setup-qa/references/qa-md-format.md` が SSOT (frontmatter の `feature` / `verification` / `last_verified_commit` / `last_verified_at`、`## N. セクション` の見出し、`- [ ] **項目名**: 期待動作`、入れ子の `自動化:`、`⏭️ スキップ:` / `❌ 失敗:`、`#### 動作確認` の details 内のエビデンスブロック)。パーサはこの文書の構文をすべて扱い、理解できない行で失敗せず、読み飛ばした行を報告する。

### QA.md の廃止後も残す性質

1. リポジトリの commit SHA を基準に「どの feature を再テストすべきか」を機械判定できる (`qa verified`)
2. PR 上で「何をどう確認したか」をレビューできる (`qa export --format pr-summary` の要約を PR body に貼る)

castle 側の置き換え (setup-qa / run-qa のフルスクラッチ #1357、release-app / auto-merge-pr のゲートと rules #1358、10 リポジトリの QA.md を import → 削除する skill #1359) は、QA 層の CLI の契約が main に入ってから castle の issue で進める。

## Web 画面

- データベースごとの表 / 一覧ビュー。プロパティ (label を含む) での絞り込みと並べ替え。行の詳細ページでの編集
- QA の入力ビュー: 参考画像 ( https://x.com/yuto_apps/status/2105243014352478550 ) のように 1 項目ずつ OK / NG / あとで + メモを入力する。進捗フッター (入力済み / 全件、OK / NG / あとで の内訳)。「全部コピー」で未入力・NG を markdown にして agent に貼れる
- Mac のブラウザと、同じ LAN の iPhone から同じ画面を開ける (`tansu serve --host 0.0.0.0`)

## MCP サーバー

- stdio で動く。CLI と同じ操作を tool として公開する
- Claude Code (`.mcp.json`)、Codex CLI、pi の 3 つに同じサーバーを登録する手順を README に書く

## 複数の Mac の同期

同期の手段は関門 1 で決める。候補は、iCloud Drive 上の SQLite / 専用の private git repo に JSON で持つ / 常時稼働の 1 台にサーバーを置き他はクライアント。DIRECTION.md「必要な機能」の同期以外の項目は 1 台の Mac で完結させ、同期は最後に足す (着手順はロードマップの親 issue)。

## 制約

- **既定は localhost だけ**: サーバーは既定で `127.0.0.1` で待ち受ける。ログインが無いため、LAN に開くのは `--host 0.0.0.0` を明示した時だけにする
- **`Host` の検査 (DNS rebinding 対策)**: HTTP API は `Host` が許可リストに無いリクエストを拒否する。許可リストは、`127.0.0.1` と `localhost` (ポート付き) に加え、`--host 0.0.0.0` の時はこのマシンのネットワークインターフェースの IP (起動時に列挙する。iPhone はこれで接続する) と、`--allow-host <ホスト名>` で明示したもの (Tailscale の MagicDNS 名など)
- **書き込みの API は CSRF も防ぐ**: 書き込み (upsert / delete / `qa record` 等) は `Host` が正当でも別のサイトからの form POST や no-cors のリクエストで届くため、`Origin` が無いか `Host` と同じオリジンの時だけ受け付け、`Sec-Fetch-Site` が `cross-site` なら拒否し、本文は `Content-Type: application/json` だけを受け付ける (別のサイトがプリフライトなしで送れる形式を受け付けない)。CLI と MCP は HTTP を通らずコアを直接呼ぶため影響しない
- **マシンの外へ出さない**: 計測・テレメトリ・実行時の外部へのリクエストを持たない。アプリが書くファイルは DB と利用記録 `$TANSU_HOME/usage.jsonl` (画面を開いた日時と CLI の実行の種類だけ。データの内容は書かない) だけで、利用記録は DIRECTION.md の判定基準の計測元になる
- **本物の QA.md をリポジトリに入れない**: private リポジトリの項目・エビデンスの URL を含むため。fixture とスクリーンショットは手書きの合成 QA.md から作る (`.claude/rules/synthetic-fixtures.md`)
- **冪等**: `qa import` と `upsert` は自然キー (取り込み元のファイルと項目名、行 ID) で upsert し、同じ入力で 2 回実行しても行が増えない。`qa record` は QA を実行した事実の記録なので、実行のたびに run を 1 行足す (同じ項目を 2 回確認すれば run は 2 行)。再試行で同じ実行が二重に入るのを防ぐ時は、呼び出し側が `--run-id <一意な ID>` を渡し、同じ `run-id` の run は 1 行にする

## インフラの決定

| 領域 | 決定 | 理由 |
| --- | --- | --- |
| DB・ストレージ | 利用者のマシンの SQLite 1 ファイル (`bun:sqlite`) | ローカルで動かす前提。ネイティブ拡張の依存を持たない |
| ホスティング | 持たない。利用者のマシンで clone から動かす | データが手元にあり、非公開のもの |
| 認証 | 持たない。既定の `127.0.0.1` での待ち受けと `Host` / `Origin` の検査で守る | 自分のマシンで 1 人が使う。LAN に開くのは明示した時だけ |
| 計測 | 手元の利用記録と DB の行数 | 外部へ送信しない制約に合う外部サービスが無い |
| 通知 | Slack の `#tansu-notification` (castle の slack-notification-setup skill で作成) | 関門の投稿先。サービスからの通知は持たない |
| アラート (GCP・Crashlytics)・課金・ストア配布・法務ドキュメント・紹介サイト | 対象外 | クラウドのプロジェクト・モバイルアプリ・支払い・外部へのデータ送信が無い |

## 検証

ビルド・テスト・画面の確認の方法は [AGENTS.md](../AGENTS.md) にある。要点: 開発マシンではビルドもブラウザでの表示も行わず、GitHub Actions が合成の fixture で行う。
