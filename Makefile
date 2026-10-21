# 引数なしの make で、サーバーを起動してブラウザで開く (人が手で動作確認するための入口)。
# 検査・テストは CI (.github/workflows/ci.yml) が行い、make には含めない。agent は開発マシンで実行しない (AGENTS.md「検証」)。
.DEFAULT_GOAL := web

# ブラウザで開く URL をサーバーの待ち受けと一致させるため、serve サブコマンドの既定のポートと同じ値にする。環境変数 TANSU_PORT で上書きできる。
# 7979 は、開発マシンで同時に動く他の dev サーバーの既定 (3000 / 5173 / 8080 / agent-timeline の 7878) と重ならない番号として選んだ
# (IANA には micromuse-ncps として登録があるが、開発マシンで動くことは無い)。既定の定義は documents/PROJECT.md「CLI (汎用)」の serve の行。
TANSU_PORT ?= 7979
export TANSU_PORT

# tansu serve が起動のたびに HTTP API のトークンを書くファイル。TANSU_HOME の既定 ~/.tansu は crates/core/src/config.rs の
# DEFAULT_HOME_DIR_NAME、ファイル名は crates/core/src/api_token.rs の TOKEN_FILE_NAME。
TANSU_TOKEN_FILE = $${TANSU_HOME:-$$HOME/.tansu}/token

.PHONY: web build-web cli build-cli

# tansu は web/dist をビルド時に埋め込む (crates/server/build.rs) ため、先に画面をビルドする。
# サーバーが前面で動くため、ブラウザは背面でトークン付きの API が応答するまで待ってから、サーバーが表示する URL と同じ
# #token= 付きの URL で開く (Rust のビルドを先に終えておき、待つのは起動だけにする)。トークン付きで待つのは、前回の起動の
# トークンのファイルを読んで開かないため。トークンはプロセスの引数 (他のユーザーも ps で見える) に載せず、標準入力で渡す
# (curl のヘッダーは --config -、ブラウザで開く URL は osascript が標準入力から読む AppleScript の open location)。
# 起動に失敗した時に待ち続けないよう、待つのは 0.2 秒 × 150 回 (30 秒。DB を開いてマイグレーションを当てる起動に十分な長さ) まで。
web: build-web
	cargo build -p tansu-cli
	(for _ in $$(seq 150); do token="$$(cat "$(TANSU_TOKEN_FILE)" 2>/dev/null)" && printf 'header = "Authorization: Bearer %s"\n' "$$token" | curl -fs -o /dev/null --config - http://127.0.0.1:$(TANSU_PORT)/api/databases && { printf 'open location "http://127.0.0.1:%s/#token=%s"\n' "$(TANSU_PORT)" "$$token" | osascript; break; }; sleep 0.2; done) &
	./target/debug/tansu serve --port $(TANSU_PORT)

# Web 画面 (web/) を Vite でビルドする。成果物 web/dist/ は tansu のビルドがバイナリに埋め込む。
build-web:
	bun install --frozen-lockfile --cwd web
	bun run --cwd web build

# CLI の単一バイナリをビルドして ~/.local/bin へ配置する (castle の skill とゲートはこの tansu を実行する)。
# cp で同じ inode を上書きすると、macOS がキャッシュした ad-hoc 署名と中身が食い違って Killed: 9 になるため、install で置き換える。
cli: build-cli
	mkdir -p $(HOME)/.local/bin
	install -m 755 target/release/tansu $(HOME)/.local/bin/tansu

# ~/.local/bin/tansu 1 つで画面が出るように、web/dist を作ってから埋め込んでビルドする。
build-cli: build-web
	cargo build --release --locked -p tansu-cli
