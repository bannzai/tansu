# 引数なしの make で、サーバーを起動してブラウザで開く (人が手で動作確認するための入口)。
# 検査・テストは CI (.github/workflows/ci.yml) が行い、make には含めない。agent は開発マシンで実行しない (AGENTS.md「検証」)。
.DEFAULT_GOAL := web

# ブラウザで開く URL をサーバーの待ち受けと一致させるため、serve サブコマンドの既定のポートと同じ値にする。環境変数 TANSU_PORT で上書きできる。
# 7979 は、開発マシンで同時に動く他の dev サーバーの既定 (3000 / 5173 / 8080 / agent-timeline の 7878) と重ならず、
# IANA のサービス名・ポート番号の登録にも無い番号として選んだ (serve サブコマンドの実装後はそちらのコメントを正にする)。
TANSU_PORT ?= 7979
export TANSU_PORT

.PHONY: web build-web cli build-cli

# サーバーが前面で動くため、ブラウザは背面で少し待ってから開く。
web:
	(sleep 1 && open http://127.0.0.1:$(TANSU_PORT)) &
	bun run ./src/cli.ts serve --port $(TANSU_PORT)

# Web 画面は Bun がソースから直接配信するため、サーバー向けの個別のビルドは無い。
build-web: build-cli

# CLI の単一バイナリをビルドして ~/.local/bin へ配置する (castle の skill とゲートはこの tansu を実行する)。
cli: build-cli
	mkdir -p $(HOME)/.local/bin
	cp dist/tansu $(HOME)/.local/bin/tansu

build-cli:
	bun run build
