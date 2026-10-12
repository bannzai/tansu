#!/usr/bin/env bash
# E2E のサーバーを起動する。TANSU_HOME を空にしてから fixtures/ の合成 QA.md を取り込み、tansu serve を前面で動かす
# (Playwright の webServer が web/ を作業ディレクトリにして実行し、テストが終わると止める)。
# 環境変数 TANSU_HOME / TANSU_PORT / TANSU_BIN は e2e/playwright.config.ts が渡す。
set -euo pipefail

# 前回の実行の DB と利用記録を残さず、毎回同じ状態から始める
rm -rf "$TANSU_HOME"
"$TANSU_BIN" qa import ../fixtures/qa/sample-notes
exec "$TANSU_BIN" serve --host 127.0.0.1 --port "$TANSU_PORT"
