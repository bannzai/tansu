#!/usr/bin/env bash
# webtunnel のセッションの初期化 (.github/workflows/browser-session.yml の setup_script の既定のパス)。
# tansu serve の HTTP API はトークンを要るため、serve が runner に書いたトークンを付けた URL (serve が表示する URL と同じ
# #token= 付き) を Chromium で開き、画面にトークンを渡した状態でセッションを始める。
# bannzai/webtunnel の runner/run-auth-setup.sh が、Chromium の起動後に WEBTUNNEL_CDP_URL を渡して実行する。
set -euo pipefail

: "${WEBTUNNEL_CDP_URL:?WEBTUNNEL_CDP_URL が未設定 (webtunnel の run-auth-setup.sh から実行する)}"

# browser-session.yml の start_command の TANSU_HOME と port に合わせる。localhost にするのは、webtunnel が start_url を
# 省いた時に開く URL (http://localhost:<port>/) と同じ origin にして、localStorage のトークンをそのまま使うため
token="$(cat "$RUNNER_TEMP/tansu-home/token")"
agent-browser --cdp "$WEBTUNNEL_CDP_URL" open "http://localhost:7979/#token=${token}"
echo "tansu の画面をトークン付きで開いた"
