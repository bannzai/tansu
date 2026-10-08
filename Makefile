# 引数なしの make で、サーバーを起動してブラウザで開く (人が手で動作確認するための入口)。
# 検査・テストは CI (.github/workflows/ci.yml) が行い、make には含めない。agent は開発マシンで実行しない (AGENTS.md「検証」)。
.DEFAULT_GOAL := web

# ブラウザで開く URL をサーバーの待ち受けと一致させるため、serve サブコマンドの既定のポートと同じ値にする。環境変数 TANSU_PORT で上書きできる。
# 7979 は、開発マシンで同時に動く他の dev サーバーの既定 (3000 / 5173 / 8080 / agent-timeline の 7878) と重ならない番号として選んだ
# (IANA には micromuse-ncps として登録があるが、開発マシンで動くことは無い)。既定の定義は documents/PROJECT.md「CLI (汎用)」の serve の行。
TANSU_PORT ?= 7979
export TANSU_PORT

.PHONY: web build-web cli build-cli

# serve サブコマンドは Web 画面の issue で実装する。それまでは、無いコマンドを呼んで黙って失敗する代わりに理由を出して止める。
# 実装後はこの target を次の 2 行に置き換える (サーバーが前面で動くため、ブラウザは背面で少し待ってから開く):
#   (sleep 1 && open http://127.0.0.1:$(TANSU_PORT)) &
#   bun run ./src/cli.ts serve --port $(TANSU_PORT)
web:
	@echo "tansu serve は未実装です (Web 画面の issue で実装する。進捗は documents/DIRECTION.md「必要な機能」)" >&2
	@exit 1

# Web 画面は Bun がソースから直接配信するため、サーバー向けの個別のビルドは無い。
build-web: build-cli

# CLI の単一バイナリをビルドして ~/.local/bin へ配置する (castle の skill とゲートはこの tansu を実行する)。
# cp で同じ inode を上書きすると、macOS がキャッシュした ad-hoc 署名と中身が食い違って Killed: 9 になるため、install で置き換える。
cli: build-cli
	mkdir -p $(HOME)/.local/bin
	install -m 755 dist/tansu $(HOME)/.local/bin/tansu

build-cli:
	bun run build
