---
status: evaluating
decision_date:
cycle_days: 14
veto_wait_hours: 12
daily_issue_cap: 3
launched_at:
---

# 方向性: tansu

## 仮説

複数のリポジトリで AI agent と人間が QA を回している開発者 (まず bannzai 自身) は、QA の項目と実行記録を 158 ファイルの QA.md (castle の setup-qa / run-qa skill の形式) に持っていて、人間が読むには不便で、全体テストの時に feature ごとにファイルを開いている。自由にプロパティを定義できる表 (Notion 風のデータベース) をローカルの SQLite に持ち、Web 画面で絞り込んで入力し、CLI と MCP で agent が読み書きできれば、QA.md を廃止して 1 つの DB を唯一の正にでき、label での横断の絞り込みと、commit を基準にした再テスト対象の機械判定ができる。汎用の DB として作るため、QA 以外の用途 (CMS) にも同じ土台を使える。

自分が使うための道具で、収益は目的にしない。需要・競合・マネタイズの事前評価 (evaluate-service-idea) は行っていない (castle の issue https://github.com/bannzai/castle/issues/1349 で代替案 (QA.md を正のまま表示ツールを足す / 静的 HTML / macOS アプリ) を比較し、bannzai が 2026-10-02 に「DB を正にして QA.md は廃止・Web アプリ・CLI と MCP の両方・既存のエビデンスも取り込む」と決めている)。

## 判定基準

| 指標 | 計測元 (skill / コマンド) | 継続のしきい値 | 打ち切り条件 | 転換の条件 |
| --- | --- | --- | --- | --- |
| 14 日間に tansu に記録した QA の実行結果 (run) の件数 (bannzai 自身と agent) | `tansu query runs --json --where 'recorded_at >= <14 日前の日付>' \| jq length` (CLI の契約は `documents/cli-contract.md`。実装前は `documents/PROJECT.md` の CLI の節) | 20 件以上 | 2 回連続で 5 件未満 | run は増えるが Web 画面を開いた日数が 0 なら、Web 画面を外して CLI と MCP だけに絞る |
| 14 日間で Web 画面を開いた日数 (bannzai 自身) | サーバーが画面の読み込みのたびに `~/.tansu/usage.jsonl` へ 1 行書く。`jq -r 'select(.event == "web_open") \| .at[0:10]' ~/.tansu/usage.jsonl \| sort -u \| wc -l` を直近 14 日分で数える | 4 日以上 | (run の件数の打ち切り条件に従う) | 上の行と同じ |
| QA.md が残っているリポジトリの数 (`~/ghq/github.com/bannzai` 配下) | `find ~/ghq/github.com/bannzai -maxdepth 4 -name QA.md -not -path '*/node_modules/*' \| xargs -n1 dirname \| cut -d/ -f1-6 \| sort -u \| wc -l` (立ち上げ時点で 10) | `qa import` の完成 (castle #1359 の着手) から 28 日で 0 | 56 日たっても 5 以上なら、移行の障害 (取り込めない記述・castle 側の skill の未整備) を issue にして転換を判断する | 残るリポジトリの理由が「QA.md の方が便利」なら、tansu を QA.md の表示・集計ツールに戻す (castle #1349 の案 3) |

## 必要な機能

- [ ] 汎用のデータベース: 自由に定義したプロパティ (text / number / select / multi-select / checkbox / url / date / relation) を持つ表を複数作れ、relation で表同士を結べる。各行は詳細ページ (markdown 本文 + プロパティ) を持つ。保存先は SQLite 1 ファイル
- [ ] CLI (汎用): `db create` / `db schema` (表とプロパティの定義)、`query --json` (絞り込み)、`upsert` / `delete` (行の読み書き)、`export` (markdown / JSON)
- [ ] QA 層のスキーマと `qa import`: project (= リポジトリ) → feature → item (項目名・期待動作・自動化ステータス `auto(flow パス) | manual(理由) | todo`・仕様 ID の紐付け) → run (結果 OK / NG / あとで / スキップ・メモ・確認日・画像 URL・検証環境・検証した commit SHA)。label は multi-select プロパティ。`qa import` は QA.md 群 (castle の `setup-qa/references/qa-md-format.md` の形式) をエビデンス・`last_verified_commit`・⏭️ / ❌ の記録ごと取り込む
- [ ] QA 層の CLI: `qa record` (agent が結果・画像 URL・commit を書く)、`qa verified --repo <owner/repo> --commit <sha>` (その commit 以降に変更された feature と未検証項目を返す。castle の release-app / auto-merge-pr のゲートが呼ぶ)、`qa export --format pr-summary` (PR body に貼る要約)。castle 側 (#1357 / #1358 / #1359) が依存する契約はここまで
- [ ] MCP サーバー (stdio): CLI と同じ操作を tool として公開する。Claude Code (`.mcp.json`) / Codex CLI / pi の 3 つに同じサーバーを登録する手順を README に書く
- [ ] Web 画面: データベースごとの表 / 一覧ビュー (プロパティと label での絞り込み・並べ替え)、行の詳細ページでの編集。QA の「1 項目ずつ OK / NG / あとで + メモを入力」のビュー、進捗フッター (入力済み / 全件、OK / NG / あとで の内訳)、「全部コピー」(未入力・NG を markdown にして agent に貼る)。同じ LAN の iPhone からも開ける
- [ ] 複数の Mac の同期: 同期の手段は関門 1 で決める (iCloud Drive 上の SQLite / 専用の private git repo に JSON で持つ / 常時稼働の 1 台にサーバーを置き他はクライアント)

MVP に入れないもの: ログイン、クラウドのホスティング、Notion との同期、他人への配布 (npm への公開)、QA.md への書き戻し。

## デザインの方向

(関門 2 で決める。参考: https://x.com/yuto_apps/status/2105243014352478550 の自作テストツール (1 項目 = 番号 + タイトル + タグ + 図解 + 「手で見ること」+ OK / NG / あとで + メモ、フッターに進捗と「全部コピー」。被テストアプリの画面と並べて使う))

## 決めたこと

| 日付 | 場面 | 決めたこと | 決めた人 |
| --- | --- | --- | --- |
| 2026-10-02 | 関門 1 の前 (castle #1349) | QA.md を正のまま残す案ではなく、DB を正にして移行後に QA.md を廃止する。ツールの形は Web アプリ。agent からは CLI と MCP の両方。castle 側の QA.md に依存する 11 箇所をすべて置き換える。既存のエビデンスも DB に取り込む | bannzai |
| 2026-10-08 | 関門 1 の前 | リポジトリ名 tansu・公開設定 public・構成 TypeScript (Bun + SQLite、Web / CLI / MCP) ( https://github.com/bannzai/IdeaMemo/issues/350 の立ち上げ情報) | bannzai |
| 2026-10-08 | 関門 1 の前 | ビルド・テスト・ブラウザの動作確認は外部のマシン (public の間は GitHub Actions と webtunnel、private にしたら Devin) で行い、開発マシンでは行わない (`/create-new-app` の起動時の指示) | bannzai |
| 2026-10-08 | 関門 1 の前 | 需要・競合・マネタイズの事前評価 (evaluate-service-idea) は行わない。自分が使う道具で、代替案の比較と採否は castle #1349 で済んでいるため | agent |
| 2026-10-08 | 関門 1 の前 | 文書・コードのコメントは日本語で書く (同じ構成の bannzai/agent-timeline が関門 1 で「OSS として扱い、ドキュメントを含めて日本語でよい」と決めた先例に合わせた)。OSS として扱うか・ライセンスは関門 1 で聞く | agent |
| 2026-10-08 | 関門 1 の前 | ログイン・クラウドのホスティング・外部への送信を持たないため、利用規約・プライバシーポリシー・紹介サイト・Search Console は作らず、README の記載で代える。GCP・Crashlytics・ストア配布・課金も対象外 | agent |
| 2026-10-08 | 関門 1 の前 | DB ファイルの既定は `~/.tansu/tansu.sqlite` (環境変数 `TANSU_DB` で差し替え)。サーバーは既定で `127.0.0.1` だけで待ち受け、LAN の iPhone から開く時は `--host 0.0.0.0` を明示する (ログインが無いため、既定で LAN に露出しない) | agent |
| 2026-10-08 | 関門 1 の前 | 本物の QA.md (private リポジトリの項目・エビデンスの URL を含む) はこの public リポジトリに入れない。テストと CI は手書きの合成 QA.md (`fixtures/`) で行い、158 ファイルの実データでの欠落ゼロの検証は castle #1359 (import → 削除の skill) の側で bannzai のマシンで行う (`.claude/rules/synthetic-fixtures.md`) | agent |

## agent に任せること

- Web 画面のライブラリの選定 (単一ページの HTML という制約の中で)、画面の細部、API の形
- SQLite のスキーマの細部 (メタスキーマの持ち方・マイグレーションの方式。規約は `.claude/rules/sqlite-database.md`)
- CLI のサブコマンドの引数の形と JSON の形 (castle 側が依存する契約は `documents/cli-contract.md` に書いて固定する)
- CI の構成、テストの書き方
