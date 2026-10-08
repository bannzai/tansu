# tansu

ローカルで動く Notion 風のデータベース / CMS。自由にプロパティを定義した表を SQLite に持ち、Web 画面で閲覧・絞り込み・入力し、AI agent からは CLI と MCP サーバーで読み書きする。最初のユースケースは複数リポジトリの QA.md の置き換え。ドキュメントとコードのコメントは日本語で書く (`documents/DIRECTION.md`「決めたこと」2026-10-08)。

## 文書

- `documents/DIRECTION.md`: なぜ作るか・何で判定するか・MVP に必要な機能の正。ここで決まっていない事項は agent が決めて「決めたこと」の表に記録する
- `documents/PROJECT.md`: 要件と制約の正 (構成・汎用の DB・CLI・QA 層・制約)。設計の決定を変える時は同じ変更の中で更新する
- `documents/cli-contract.md` (未作成。QA 層の CLI の issue で作る): castle の skill とゲートが依存する CLI の契約 (引数・JSON の形・exit code)。作った後は、契約を変える時に同じ変更の中で更新する
- `.claude/rules/sqlite-database.md`: SQLite のデータの持ち方。`.claude/rules/synthetic-fixtures.md`: 本物の QA.md をリポジトリに入れない

## 検証

ビルド・テスト・ブラウザを開く作業は、開発マシンの負荷を避けるため、開発マシンではなく外部のマシンで行う。

- tansu は public リポジトリのため、外部のマシンは GitHub Actions (`.github/workflows/ci.yml`。public リポジトリは無料) を使う。simtunnel は iOS / macOS アプリ用で対象外。リポジトリを private にした場合は GitHub Actions の代わりに Devin のセッション (devin-macos-e2e skill の経路) を使う
- 開発マシンで実行しないもの: `bun install` (`--lockfile-only` なし)、ビルド、テスト、dev サーバー、Playwright、ローカルのブラウザ (`--cdp` なしの agent-browser)。ファイルの編集・`git`・`gh`・`bun install --lockfile-only` (インストールせず `bun.lock` だけを更新する) は開発マシンで行ってよい
- ブランチを push して PR を作ると、PR ごとに CI が動く。PR の無いブランチで動かす時: `gh workflow run ci.yml --ref <ブランチ>`
- CI の手順が検証コマンドになる: `bun install --frozen-lockfile` → `bun run lint` (Biome の lint と整形の検査) → `bun run typecheck` → `bun test` → `bun run build`
- 引数なしの `make` は、人が手で動作確認するための入口で、サーバーを起動して http://127.0.0.1:7979 をブラウザで開く (`Makefile` の `web`)。`serve` サブコマンドが実装される (Web 画面の issue) までは、未実装の案内を出して exit 1 で止まる。`make cli` は CLI の単一バイナリを `~/.local/bin/tansu` に置く。検査・テストは含めず CI が行う。agent は開発マシンで実行しない
- 結果を待って読む: `gh pr checks <PR> --watch`。失敗は `gh run view <run ID> --log-failed`
- 画面の確認: Web 画面の issue で、Playwright の E2E (`e2e/`) を CI の `e2e` job に足し、スクリーンショットを `e2e-screenshots` artifact として上げる構成にする。以後、画面の振る舞いを足す時は E2E でその画面を操作してスクリーンショットを保存し、`gh run download <run ID> -n e2e-screenshots -D ./tmp/e2e-screenshots-<run ID>` で取得して PNG を Read して判断する。そのスクリーンショットが変更の証拠になる
- CI には本物の QA.md も DB も無い。テストと E2E は `TANSU_HOME` を一時ディレクトリにし (DB と利用記録がその下に入る)、`fixtures/` の手書きの合成 QA.md を取り込んで確認する (`.claude/rules/synthetic-fixtures.md`)
- テストではなく手で画面を操作して確かめたい時は、GitHub Actions の runner 上の Chromium を操作する `webtunnel` skill を使う。caller workflow (`.github/workflows/browser-session.yml`。runner で `fixtures/` を取り込んだサーバーを起動する) は Web 画面の issue で足し、Secrets (`TS_OIDC_CLIENT_ID` / `TS_OIDC_AUDIENCE`) の登録は「ユーザー作業の一覧」issue に載せる。public リポジトリでは録画とスクリーンショットの artifact が公開されるため、本物の QA.md を取り込んだ画面を表示しない
- 本物の QA.md (10 リポジトリ・158 ファイル) での取り込みの確認は CI では行わず、castle の import → 削除の skill (https://github.com/bannzai/castle/issues/1359 ) が bannzai のマシンで行う

<!-- ai-review-config begin -->
<!--
このブロックは自動生成です。直接編集せず、テンプレートを更新してから再生成してください。
内容は AI コードレビュー時の挙動指示であり、コードベース自体への規約ではありません。
-->

## レビュー時の応答スタイル

- 応答は日本語で行う

## レビュー範囲外

以下は自動レビューで指摘しない (別の検出経路があるため):

- コンパイルエラー・型エラー (ローカル/CI のビルドで検出される)
- Lint/フォーマット違反 (リンター・フォーマッターで検出される)
<!-- ai-review-config end -->
