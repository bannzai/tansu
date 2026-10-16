# tansu

ローカルで動く Notion 風のデータベース / CMS。自由にプロパティを定義した表を複数持ち、Web 画面で閲覧・絞り込み・入力し、AI agent からは CLI と MCP サーバーで読み書きする。最初のユースケースは、複数リポジトリの QA の項目と実行記録 (QA.md) の置き換え。

- 何を作るか・何で判定するか: [documents/DIRECTION.md](documents/DIRECTION.md)
- 要件と制約: [documents/PROJECT.md](documents/PROJECT.md)
- 開発の進め方 (検証は GitHub Actions で行う): [AGENTS.md](AGENTS.md)

## 使い方

Rust (`rust-toolchain.toml` の版) が要る。入れ方は [rustup](https://rustup.rs/) を入れ、clone 後に `rustup toolchain install` を実行すると `rust-toolchain.toml` の版が入る。`web/` を触る時は Bun (`web/.bun-version` の版) も要る。

libsql の crate (`Cargo.lock` の `libsql-ffi`) は、同梱の SQLite の C ソースをビルドするため C コンパイラが要る (macOS は Xcode Command Line Tools)。<!-- source: https://github.com/tursodatabase/libsql の libsql-ffi/build.rs が cc::Build で bundled/src/sqlite3.c をコンパイルし、libsql-ffi の build-dependencies に cc と cmake がある (Cargo.lock の libsql-ffi の依存にも cc と cmake)。crates.io の説明は "Native bindings to libSQL" だけ -->

```sh
git clone https://github.com/bannzai/tansu.git
cd tansu
rustup toolchain install   # rust-toolchain.toml の版を入れる
make cli   # CLI を ~/.local/bin/tansu に置く (~/.local/bin が PATH に無ければ足す)
tansu --version
```

### CLI

データは `TANSU_HOME` (既定 `~/.tansu`) の下の `tansu.sqlite` に入る。出力は JSON で、失敗すると stderr に 1 行を出して exit 1 で終わる。引数の書き方の誤り (足りない引数・知らないオプション) は、コマンドラインの解析 (clap) が使い方を複数行で出して exit 2 で終わる。

```sh
# 表を作り、プロパティを足す。型は text / number / checkbox / url / date / select / multi_select / relation
tansu db create projects
tansu db create tasks
tansu db schema tasks \
  --add title:text --add points:number --add done:checkbox --add link:url --add due:date \
  --add status:select:todo,done --add labels:multi_select:bug,ui --add project:relation:projects
tansu db schema projects --add repo:text --unique repo   # unique: 同じ値の行を 2 つ持てない
tansu db schema tasks --remove link                      # 行の値からも消える
tansu db list

# 行を書く。id があればその行、無ければ unique のプロパティの値が一致する行を更新し、無ければ足す
tansu upsert projects --json '{"values": {"repo": "owner/repo"}}'
tansu upsert tasks --json '{"id": "task-1", "body": "本文 (markdown)", "values": {"title": "Write docs", "status": "todo", "labels": ["ui"], "project": ["<projects の行 ID>"]}}'
echo '{"id": "task-1", "values": {"status": "done"}}' | tansu upsert tasks   # stdin からも読む。書いたプロパティだけが変わる

# 絞り込み。--where は複数指定でき、すべてに一致する行を返す (multi_select / relation は「含む」)。--sort が無ければ追加した順
tansu query tasks --json --where status=done --where 'title^=Write' --sort points:desc --limit 10

tansu delete tasks task-1   # relation でこの行を参照している値からも外す
tansu export tasks --format markdown
```

select / multi_select の値は選択肢の name で読み書きする (DB には選択肢の ID で保存する)。同じ `upsert` を 2 回実行しても行は増えない。`id` も unique のプロパティの値も無い `upsert` はエラーになる。行の JSON の形は [documents/data-model.md](documents/data-model.md)「CLI の入出力」にある。

### QA 層 (QA.md の取り込みと QA の記録)

castle の setup-qa / run-qa skill の形式の QA.md を、4 つの表 (project / feature / item / run) に取り込む。表が無ければ作る。

```sh
tansu qa import ~/ghq/github.com/owner/repo                   # リポジトリのルート (配下の QA.md をすべて探す)
tansu qa import lib/features/notes/QA.md --json               # QA.md のファイル。--json で件数と行の扱いの内訳を JSON で出す
tansu qa import ~/ghq/github.com/owner/repo --dry-run --json  # 書かずに同じ出力を出す
tansu query run --json --where source=import                  # 取り込んだ記録
```

同じ QA.md を 2 回取り込んでも行は増えない。`--json` の `counts` は入力に含まれる project / feature / item / run の数で、2 回目や `--dry-run` でも同じ値になる (新しく足した行の数ではない)。`skipped` は構造として読めず feature・item・run の本文に残した行 (`file` / `line` / `content` / `reason`)、`unaccounted` は本文にも入らなかった行で、空なら QA.md の内容はすべて DB から読める (形は [documents/cli-contract.md](documents/cli-contract.md))。取り込みの規則 (project の名前の決め方、run の結果と記録日時) は [documents/data-model.md](documents/data-model.md)「QA 層」にある。

QA を実行した結果の記録と、commit を基準にした再テストの判定:

```sh
# 結果を記録する (実行のたびに run が 1 行増える。--run-id を渡すと同じ ID の run は 1 行)
tansu qa record --repo owner/repo --feature notes --item-name 'ノートを保存できる' \
  --result ok --commit "$(git rev-parse HEAD)" --image-url https://example.com/save.png --json

# その commit で再テストが要る feature と未検証の項目 (exit 0 = 不要、1 = 要る、2 = エラー)
tansu qa verified --repo owner/repo --commit "$(git rev-parse HEAD)" --json

# PR body に貼る確認内容の表 (--since の子孫の commit で記録した run だけ)
tansu qa export --repo owner/repo --format pr-summary --since "$(git merge-base origin/main HEAD)"
```

引数・JSON の形・exit code・判定の規則は [documents/cli-contract.md](documents/cli-contract.md) にある。

Web 画面 (データベースの表ビュー・行の詳細・QA の入力ビュー) は `tansu serve` で開く:

```sh
make cli                                          # web/ をビルドし、画面を埋め込んだ tansu を ~/.local/bin に置く
tansu serve                                       # http://127.0.0.1:7979 で待ち受ける (このマシンからだけ開ける)
tansu serve --host 0.0.0.0                        # 同じ LAN の iPhone から http://<この Mac の IP>:7979 で開く
tansu serve --allow-host mac.tailnet.ts.net       # Host の許可リストに名前を足す (Tailscale の MagicDNS 名など)
```

ログインが無いため、LAN に開くのは `--host 0.0.0.0` を明示した時だけ。`Host` が許可リストに無いリクエストと、別のサイトからの書き込みは拒否する (規則は [documents/PROJECT.md](documents/PROJECT.md)「制約」)。QA の入力ビューは、project の checkout の HEAD の commit で run を記録する。

### MCP サーバー

`tansu mcp` は stdio の MCP サーバーで、CLI のサブコマンドと 1 対 1 の 11 の tool (`db_list` / `db_create` / `db_schema` / `query` / `upsert` / `delete` / `export` / `qa_import` / `qa_record` / `qa_verified` / `qa_export`) を公開する。入力・出力・失敗の扱いは [documents/cli-contract.md](documents/cli-contract.md)「MCP サーバー」にある。`tansu` が PATH にある (`make cli`) 前提で、3 つのツールに同じサーバーを登録する。DB と利用記録の置き場所を変える時は、各設定の `env` に `TANSU_HOME` を書く。

**Claude Code** ( https://code.claude.com/docs/en/mcp ): プロジェクトの `.mcp.json` に書く。`type` の無い項目は stdio のサーバーとして読まれる。

```json
{"mcpServers": {"tansu": {"command": "tansu", "args": ["mcp"]}}}
```

コマンドで足す時: `claude mcp add --transport stdio --scope project tansu -- tansu mcp`

**Codex CLI** ( https://developers.openai.com/codex/mcp 。2026-10-12 時点で https://learn.chatgpt.com/docs/extend/mcp?surface=cli へ転送される): `~/.codex/config.toml` (信頼したプロジェクトの `.codex/config.toml` でもよい) に書く。

```toml
[mcp_servers.tansu]
command = "tansu"
args = ["mcp"]
```

コマンドで足す時: `codex mcp add tansu -- tansu mcp`

**pi** ( https://pi.dev/docs/latest/mcp ): `~/.pi/agent/mcp.json` (プロジェクトの `.pi/mcp.json` でもよい。信頼したプロジェクトだけ読まれる) に、Claude Code と同じ形で書く。

```json
{"mcpServers": {"tansu": {"command": "tansu", "args": ["mcp"]}}}
```

コマンドで足す時: `pi mcp add tansu -- tansu mcp` (プロジェクトの設定に書く時は `-l` を付ける)

<!-- source: 2026-10-12 に各ページを取得して確かめた。code.claude.com/docs/en/mcp: 「Claude Code reads an entry with no `type` as a stdio server」、`claude mcp add [options] <name> -- <command> [args...]`、`--scope project` は `.mcp.json` に書く。learn.chatgpt.com/docs/extend/mcp?surface=cli: 「By default this is `~/.codex/config.toml`, but you can also scope MCP servers to a project with `.codex/config.toml` (trusted projects only)」「Configure each MCP server with a `[mcp_servers.<server-name>]` table」、`codex mcp add <server-name> ... -- <stdio server-command>`。pi.dev/docs/latest/mcp: 「Pi reads user-level servers from `~/.pi/agent/mcp.json`」「project servers from `.pi/mcp.json`」(project trust の後)、トップレベルは `mcpServers`、stdio は `command` / `args` / `env` / `cwd`、「Add `--local` or `-l` to write the project configuration instead」 -->

## データの扱い

tansu は計測を持たず、ログインもサーバーを各 Mac の localhost で動かす間は持たず、外部への送信は利用者が選んだ Turso Cloud の DB への接続だけ (ローカルを選べば無い) という設計で作る (この時点で動くのは CLI の表の読み書き・QA 層の CLI・Web 画面・MCP サーバー。Web 画面は文字を端末のフォントで描き、本文の markdown の画像は読み込まずにリンクで出すため、画面を開いても外部へのリクエストは起きない)。データの置き場所は起動時に選ぶ (`TANSU_DB_URL`。`documents/PROJECT.md`「DB の置き場所」): 既定はこのマシンの SQLite ファイル (`TANSU_HOME` (既定 `~/.tansu`) の下の `tansu.sqlite`。`file:<パス>` で別の場所にもできる。実行中は付随ファイル `-wal` / `-shm` もできる) で、この時はマシンの外へ何も送らない。Turso Cloud の DB (`TANSU_DB_URL=libsql://...`) を選ぶと、データがその DB に保存され、外部への通信はその接続だけになる (複数の Mac で同じ DB を開けば同じデータを使える)。どちらを選んでもこのマシンに書くファイルは利用記録 (`$TANSU_HOME/usage.jsonl`。画面を開いた日時と CLI・MCP の tool の実行の種類だけ。データの内容は書かず、どこにも送らない)。サーバーは既定で `127.0.0.1` だけで待ち受ける。利用規約・プライバシーポリシーはこの記載で代える。

Turso Cloud の DB を使う手順 (Turso のアカウント・DB・トークンの作成は利用者が行う):

1. Turso の CLI でログインし (`turso auth login`。アカウントが無ければ `turso auth signup`)、DB を作る: `turso db create <DB 名>`
2. 接続先の URL を `turso db show <DB 名> --url` で、トークンを `turso db tokens create <DB 名>` で出す
3. tansu を使うディレクトリの `.envrc` (git 管理外) に書き、`direnv allow` する。tansu は起動のたびにこの 2 つを読み、無ければローカルのファイルを使う

```sh
export TANSU_DB_URL=libsql://...   # 2 の URL
export TANSU_DB_AUTH_TOKEN=...     # 2 のトークン
```

`TANSU_DB_URL` が `libsql://` でトークンが無い時と、接続できない時は、stderr に理由を 1 行出して exit 1 で止まる。初めて開いた時に表を作り、2 回目からは作り直さない。ローカルのファイルのデータは Turso に移さない (別の DB として始まる)。

<!-- source: 2026-10-13 に取得して確かめた。docs.turso.tech/quickstart: 「turso auth signup」「turso auth login」、libSQL の DB は「turso db create my-db」。docs.turso.tech/cli/db/show: `turso db show <database-name> [flags]`、`--url`「Show URL for the database HTTP API.」。docs.turso.tech/cli/db/tokens/create: `turso db tokens create <database-name> [flags]`、`--expiration`「can be `never` or a value in days」。docs.turso.tech/sdk/http/reference: 「`libsql://` for libSQL databases」。接続とマイグレーションの挙動は crates/core/src/db/mod.rs と .claude/rules/sqlite-database.md -->
