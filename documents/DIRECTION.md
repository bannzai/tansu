---
status: building
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
| 14 日間に tansu に記録した QA の実行結果 (run) の件数 (bannzai 自身と agent) | `tansu query run --json --where "recorded_at>=$(date -v-13d +%F)" --where 'source=record' \| jq length` (当日を含む 14 日) (`qa import` で取り込んだ過去の記録 (`source=import`) は数えない。データベース `run` とプロパティ `recorded_at` / `source` は `documents/PROJECT.md`「QA 層」の表。CLI の契約は `documents/cli-contract.md`) | 20 件以上 | 2 回連続で 5 件未満 | run は増えるが Web 画面を開いた日数が 0 なら、Web 画面を外して CLI と MCP だけに絞る |
| 14 日間で Web 画面を開いた日数 (bannzai 自身) | サーバーが画面の読み込みのたびに `~/.tansu/usage.jsonl` へ 1 行書く。`jq -r --arg since "$(date -v-13d +%F)" 'select(.event == "web_open" and .at[0:10] >= $since) \| .at[0:10]' ~/.tansu/usage.jsonl \| sort -u \| wc -l` で当日を含む 14 日の日付の数を数える | 4 日以上 | (run の件数の打ち切り条件に従う) | 上の行と同じ |
| QA.md が残っているリポジトリの数 (`~/ghq/github.com/bannzai` 配下) | `find ~/ghq/github.com/bannzai -maxdepth 4 -name QA.md -not -path '*/node_modules/*' -not -path '*/bannzai/tansu/*' \| sed -E 's#^.*/github\.com/bannzai/([^/]+)/.*$#\1#' \| sort -u \| wc -l` (リポジトリ名で数える。立ち上げ時点で 10) | `qa import` の完成 (castle #1359 の着手) から 28 日で 0 | 56 日たっても 5 以上なら、移行の障害 (取り込めない記述・castle 側の skill の未整備) を issue にして転換を判断する | 残るリポジトリの理由が「QA.md の方が便利」なら、tansu を QA.md の表示・集計ツールに戻す (castle #1349 の案 3) |

## 必要な機能

- [ ] 汎用のデータベース: 自由に定義したプロパティ (text / number / select / multi-select / checkbox / url / date / relation) を持つ表を複数作れ、relation で表同士を結べる。各行は詳細ページ (markdown 本文 + プロパティ) を持つ。保存先は libSQL の DB 1 つ (ローカルを選んだ時は SQLite 1 ファイル、クラウドを選んだ時は Turso Cloud の DB 1 つ)
- [ ] CLI (汎用): `db create` / `db schema` (表とプロパティの定義)、`query --json` (絞り込み)、`upsert` / `delete` (行の読み書き)、`export` (markdown / JSON)
- [ ] QA 層のスキーマと `qa import`: project (= リポジトリ) → feature → item (項目名・期待動作・自動化ステータス `auto(flow パス) | manual(理由) | todo`・仕様 ID の紐付け) → run (結果 OK / NG / あとで / スキップ・メモ・確認日・画像 URL・検証環境・検証した commit SHA)。label は multi-select プロパティ。`qa import` は QA.md 群 (castle の `setup-qa/references/qa-md-format.md` の形式) をエビデンス・`last_verified_commit`・⏭️ / ❌ の記録ごと取り込む
- [ ] QA 層の CLI: `qa record` (agent が結果・画像 URL・commit を書く)、`qa verified --repo <owner/repo> --commit <sha>` (その commit 以降に変更された feature と未検証項目を返す。castle の release-app / auto-merge-pr のゲートが呼ぶ)、`qa export --repo <owner/repo> --format pr-summary` (PR body に貼る要約)。castle 側 (#1357 / #1358 / #1359) が依存する契約はここまで
- [x] MCP サーバー (stdio): CLI と同じ操作を tool として公開する。Claude Code (`.mcp.json`) / Codex CLI / pi の 3 つに同じサーバーを登録する手順を README に書く
- [ ] Web 画面: データベースごとの表 / 一覧ビュー (プロパティと label での絞り込み・並べ替え)、行の詳細ページでの編集。QA の「1 項目ずつ OK / NG / あとで + メモを入力」のビュー、進捗フッター (入力済み / 全件、OK / NG / あとで の内訳)、「全部コピー」(未入力・NG を markdown にして agent に貼る)。同じ LAN の iPhone からも開ける
- [ ] 複数の Mac の同期: DB の置き場所を Turso Cloud (libSQL) にした時は、各 Mac が同じクラウドの DB を開くので同期の処理を持たない (2026-10-11 の決定)。ローカルの SQLite ファイルを選んだ時は同期しない

MVP に入れないもの: Notion との同期、他人への配布 (crates.io・Homebrew 等への公開)、QA.md への書き戻し。ログインとクラウドのホスティングも入れない (サーバーは各 Mac の localhost で動かす。2026-10-11 の決定。クラウドに置く案は判定基準を通ってから検討する)。

## デザインの方向

- 今の TypeScript × React の Web 画面は、`documents/design/mock-2026-10-11/` のモック (メイン・行の詳細・QA の結果入力・スマホ幅の 4 画面) のとおりに作る。bannzai がローカルで使う分にはこれで足りる (2026-10-12 の関門 2 の返答)
- 将来の Rust × Wasm の画面は Notion のような見た目と操作感にする (同じ返答)
- 参考: https://x.com/yuto_apps/status/2105243014352478550 の自作テストツール (1 項目 = 番号 + タイトル + タグ + 図解 + 「手で見ること」+ OK / NG / あとで + メモ、フッターに進捗と「全部コピー」。被テストアプリの画面と並べて使う)

## 決めたこと

| 日付 | 場面 | 決めたこと | 決めた人 |
| --- | --- | --- | --- |
| 2026-10-02 | 関門 1 の前 (castle #1349) | QA.md を正のまま残す案ではなく、DB を正にして移行後に QA.md を廃止する。ツールの形は Web アプリ。agent からは CLI と MCP の両方。castle 側の QA.md に依存する 11 箇所をすべて置き換える。既存のエビデンスも DB に取り込む | bannzai |
| 2026-10-08 | 関門 1 の前 | リポジトリ名 tansu・公開設定 public・構成 TypeScript (Bun + SQLite、Web / CLI / MCP) ( https://github.com/bannzai/IdeaMemo/issues/350 の立ち上げ情報) | bannzai |
| 2026-10-08 | 関門 1 の前 | ビルド・テスト・ブラウザの動作確認は外部のマシン (public の間は GitHub Actions と webtunnel、private にしたら Devin) で行い、開発マシンでは行わない (`/create-new-app` の起動時の指示) | bannzai |
| 2026-10-08 | 関門 1 の前 | 需要・競合・マネタイズの事前評価 (evaluate-service-idea) は行わない。自分が使う道具で、代替案の比較と採否は castle #1349 で済んでいるため | agent |
| 2026-10-08 | 関門 1 の前 | 文書・コードのコメントは日本語で書く (同じ構成の bannzai/agent-timeline が関門 1 で「OSS として扱い、ドキュメントを含めて日本語でよい」と決めた先例に合わせた)。OSS として扱うか・ライセンスは関門 1 で聞く | agent |
| 2026-10-08 | 関門 1 の前 | ログイン・クラウドのホスティング・外部への送信を持たないため、利用規約・プライバシーポリシー・紹介サイト・Search Console は作らず、README の記載で代える。GCP・Crashlytics・ストア配布・課金も対象外 (「外部への送信を持たない」はローカルを選んだ時の話に 2026-10-11 の行で縮めた。法務ドキュメントを作らない根拠は `documents/PROJECT.md`「インフラの決定」) | agent |
| 2026-10-08 | 関門 1 の前 | DB ファイルの既定は `~/.tansu/tansu.sqlite` (環境変数 `TANSU_DB` で差し替え)。サーバーは既定で `127.0.0.1` だけで待ち受け、LAN の iPhone から開く時は `--host 0.0.0.0` を明示する (ログインが無いため、既定で LAN に露出しない) (`TANSU_DB` は 2026-10-11 の行で `TANSU_DB_URL` に置き換えた) | agent |
| 2026-10-08 | 関門 1 の前 | tansu が書くファイルはすべて `TANSU_HOME` (既定 `~/.tansu`) の下に置き、DB だけ `TANSU_DB` で上書きできる。テストと CI は `TANSU_HOME` を一時ディレクトリにする (上の行の `TANSU_DB` だけを差し替える書き方は、利用記録が実際のホームに書かれるため改めた) (`TANSU_DB` は 2026-10-11 の行で `TANSU_DB_URL` に置き換えた) | agent |
| 2026-10-08 | 関門 1 の前 | 本物の QA.md (private リポジトリの項目・エビデンスの URL を含む) はこの public リポジトリに入れない。テストと CI は手書きの合成 QA.md (`fixtures/`) で行い、158 ファイルの実データでの欠落ゼロの検証は castle #1359 (import → 削除の skill) の側で bannzai のマシンで行う (`.claude/rules/synthetic-fixtures.md`) | agent |
| 2026-10-11 | 関門 1 (チャット) | 長期の構想は「Rust × Wasm でガリガリに最適化した Notion 風のサービス」で、tansu はその最初の実装。最初のユースケースは bannzai 自身の QA DB。bannzai の既存リポジトリ・IdeaMemo にこの構想の記録は無く (agent が 2026-10-11 に検索)、ここが初出 | bannzai |
| 2026-10-11 | 関門 1 (チャット) | CLI は Rust。Web 画面は今は TypeScript × React で書き、将来 Wasm (Rust のコア) を組み込んで最適化する。コア (スキーマ・問い合わせ・QA.md のパーサ) は最初から Rust の crate にし、Wasm はその build target として後から足す (書き直しにしない)。立ち上げ PR #1 の Bun + TypeScript の雛形は作り直す | bannzai (構成の細部は agent) |
| 2026-10-11 | 関門 1 (チャット) | DB の置き場所は、ローカルの SQLite ファイルと Turso Cloud (libSQL) の両方に対応し、利用者が起動時に選ぶ。クラウドを選んだ時はデータが Turso に載るので、2026-10-08 の「外部への送信を持たない」(上の法務ドキュメントの行と、`documents/PROJECT.md`「制約」の旧「マシンの外へ出さない」) はローカルを選んだ時だけに縮める | bannzai |
| 2026-10-11 | 関門 1 (チャット) | bannzai が PR #3 のマージ後に「よろしく」と返し、未回答の項目は agent が可逆な方を選んで決めることになった (下の 4 行) | bannzai |
| 2026-10-11 | 関門 1 (agent) | サーバー (`tansu serve`) は各 Mac の localhost で動かす。DB だけ Turso Cloud に置けるので複数の Mac の同期はそれで足りる。ログイン・ホスティング・CD・法務ドキュメントは不要のまま。クラウドにサーバーを置く案は追加で足せる (可逆) ため、判定基準を通ってから検討する | agent |
| 2026-10-11 | 関門 1 (agent) | OSS としては扱わず、LICENSE は足さない (後から MIT を足すのは容易で、外すのは難しいため)。文書・コメントは日本語のまま | agent |
| 2026-10-11 | 関門 1 (agent) | 必要な機能は 1〜7 のまま (同期の項目は Turso Cloud の接続に置き換え) | agent |
| 2026-10-11 | 関門 1 (agent) | データモデルは `documents/data-model.md` のとおり: 行の値は JSON 1 列 (EAV にしない)、値は自身の型を持つ、relation は片方向、run は汎用の行、ID は短い文字列、QA の automation は select + text の 2 列、property に unique、select は保存 ID・CLI の入出力 name | agent |
| 2026-10-11 | 関門 1 (チャット) の後 | 置き場所の指定は `TANSU_DB_URL` (未設定ならローカルのファイル。2026-10-08 の `TANSU_DB` はこれに置き換える)。Turso の embedded replica は CLI・MCP・serve が同じ DB を開く構成と合わないため使わず、ローカルのファイルかクラウド直結の 2 択にする (`documents/PROJECT.md`「DB の置き場所」) | agent |
| 2026-10-11 | 関門 1 (チャット) | リポジトリ名は当面 tansu のまま。よりよい名前にいずれ変える (時期は未定) | bannzai |
| 2026-10-11 | 汎用の DB と CLI の実装 (#6) | CLI の行の JSON は `values` をプロパティ名・選択肢の name で表す (`documents/data-model.md`「CLI の入出力」)。`upsert` は `id` か unique のプロパティの値で書く行を決め、どちらも無い入力はエラーにする (「すべての書き込みを冪等にする」に合わせた。後からキー無しの追加を許すのは呼び出し側を壊さないが、逆は壊すため厳しい方にした)。`delete` は行が無くても成功にする。生成する ID は `-` を含まない 21 文字 (CLI の引数でオプションと解釈されないように) | agent |
| 2026-10-12 | QA 層のスキーマと `qa import` (#9) | QA 層専用の Rust の型は持たず、汎用層の `Row` をそのまま使う。取り込んだ run は `run_id = import:<item の行 ID>` で 1 項目 1 行にし、`recorded_at` はエビデンスの確認日 → frontmatter の `last_verified_at` → 値なし の順 (取り込んだ時刻は書かない。2 回目の取り込みで行が変わらないように)。`自動化:` の行が無い項目 (root QA.md の横断確認項目) の `automation` は値なし。project の `repo` は、取り込むルートが git のルートそのものの時だけ remote から取り、それ以外はディレクトリ名。root QA.md の既知の散文セクション (対象環境・起動方法・実行ナレッジ等) は読み飛ばした行として報告せず、`## N.` のセクションと仕様チェックリストと frontmatter の中の解釈できない行だけを報告する (規則は `documents/data-model.md`「QA 層」) | agent |
| 2026-10-12 | 関門 2 (チャット) | 今の Web 画面は `documents/design/mock-2026-10-11/` のモックのままでよい (「とりあえず私のローカルで使う分にはそれでいい」)。将来の Rust × Wasm の画面は Notion のような見た目と操作感にする | bannzai |
| 2026-10-12 | QA 層のスキーマと `qa import` (#9) | `TANSU_DB_URL=file::memory:` (インメモリ) は非対応のエラーにする (ファイルのパスではなく、WAL にも切り替えられない。#14 のレビューの指摘) | agent |
| 2026-10-12 | QA 層の CLI (#10) | `qa record --image-url` は複数指定でき、最初の URL を run の `image_url` (url 型の 1 値。schema は変えない) に、すべてを run の本文に markdown の画像で書く (`qa import` も最初の画像だけを `image_url` に入れている)。`qa verified` の feature のディレクトリは、feature に足したプロパティ `path` (QA.md があったディレクトリ) で持つ。ルート直下の QA.md の feature は diff せず、`last_verified_commit` が無い・リポジトリに無い feature はディレクトリの全ファイルを再テストの対象にする (castle の run-qa の `affected_features.sh` の unverified / stale-commit と同じ扱い)。`unverified_items` は run が 1 つも無い item で、NG / あとで だけの item は含めない。`--diff-base` はその commit から変わった feature だけを判定の対象にする絞り込みにした (PR が触っていない feature の古い未検証で PR のゲートを止めないため)。`qa export --since` は since の子孫の commit の run だけを出す。利用記録は CLI のすべてのサブコマンドの実行で 1 行足し、書けなくても本来の操作は止めない (規則は `documents/cli-contract.md`) | agent |
| 2026-10-12 | MCP サーバー (#11) | MCP の tool の利用記録の `event` は `mcp_<tool 名>` にし、CLI の実行と見分けられるようにする (agent が MCP から記録した分を数えるため)。tool の失敗は CLI の stderr と同じ理由を本文にした `isError: true` の結果で返し、`qa_verified` は再テストが要る時も成功の結果で返して `ok` で判定させる (MCP には exit code が無いため)。サーバーは状態を持たず、tool の呼び出しのたびに設定を読んで DB を開く (CLI の 1 回の実行と同じにし、環境変数の誤りでサーバーごと落とさない)。入出力の規則は `documents/cli-contract.md`「MCP サーバー」 | agent |
| 2026-10-12 | Web 画面と `tansu serve` (#12) | `web/dist` はビルド時にバイナリへ埋め込む (`crates/server/build.rs`。`web/dist` 無しでビルドした tansu は画面の代わりに案内を返す)。HTTP API は行の一覧・upsert・選択肢の追加・QA の入力ビューの内容・`qa record`・`web_open` だけで、表の絞り込み (テキストは部分一致) と並べ替えは画面が行う (`tansu query` に部分一致と select・relation の並べ替えが無く、CLI の契約を広げないため)。QA の入力ビューの「入力済み」は、項目の最後の run の commit が project の checkout の HEAD と同じこと (今の commit の QA を回す画面のため)。画面から記録した run の `run_id` は `web:<ページを開いた時の ID>:<commit>:<item の行 ID>` で、同じページ・同じ commit で選び直すと run を足さずに置き換え、別の commit での再検証は別の run にする。画面は Web フォント・本文の画像を読み込まない (外部への送信を DB の接続だけにする制約) | agent |
| 2026-10-13 | Turso Cloud の接続 (#13) | マイグレーションの適用済みの判定は、ローカル・リモートとも `PRAGMA user_version` から `schema_migration` テーブルに移す (Turso Cloud で `user_version` が読み取り専用のため。経路を 1 つにして PR の CI のローカルのテストで同じコードを通す)。リモートのテストは CI 用の DB に固定名のデータベース `ci-remote` を置き、run ごとに一意の行だけを書いて消す (規則は `.claude/rules/sqlite-database.md`) | agent |
| 2026-10-16 | `qa import` の欠落をなくす (#29) | 2026-10-12 の #9 の行のうち「1 項目 1 run (`run_id = import:<item の行 ID>`)」と「root QA.md の既知の散文セクションを報告しない」を改める。run はエビデンスの確認日・再確認日ごとに 1 行 (`run_id = import:<item の行 ID>:<確認の順番>`) にし、項目の結果・⏭️ / ❌ の本文・`last_verified_commit` は `recorded_at` の順で最後の確認の run にだけ書く (QA.md は確認ごとの結果を持たず、前の確認の結果を作らないため)。記録の無い (未チェックで ⏭️ / ❌ も無い) 項目の確認は run にせず item の本文に残す (Issue の「確認日ごとに run」と違うが、run を作ると `qa verified` の `unverified_items` から外れ、再テスト待ちの項目がゲートを通ってしまうため)。散文のセクションを含め、構造として読めない行はすべて feature・item・run の本文に残して `skipped` (意味を「本文に残した行」に改める) に報告し、どこにも入らなかった行を `unaccounted` に出す。feature の本文の末尾に QA.md の原文を fenced code block で持つ (専用のプロパティにすると表のセルに全文が出るため)。同じ名前の項目は名前を変えずに別の item にし、`qa record --item-name` が 2 つ以上に当たる時は記録せず失敗させる (規則は `documents/data-model.md`「QA 層」、出力の形は `documents/cli-contract.md`) | agent |

## agent に任せること

- Web 画面のライブラリの選定 (TypeScript × React の中で)、画面の細部、API の形
- SQLite のスキーマの細部 (メタスキーマの持ち方・マイグレーションの方式。規約は `.claude/rules/sqlite-database.md`)
- CLI のサブコマンドの引数の形と JSON の形 (castle 側が依存する契約は `documents/cli-contract.md` に書いて固定する)
- CI の構成、テストの書き方
