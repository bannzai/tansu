# tansu

ローカルで動く Notion 風のデータベース / CMS。自由にプロパティを定義した表を複数持ち、Web 画面で閲覧・絞り込み・入力し、AI agent からは CLI と MCP サーバーで読み書きする。最初のユースケースは、複数リポジトリの QA の項目と実行記録 (QA.md) の置き換え。

- 何を作るか・何で判定するか: [documents/DIRECTION.md](documents/DIRECTION.md)
- 要件と制約: [documents/PROJECT.md](documents/PROJECT.md)
- 開発の進め方 (検証は GitHub Actions で行う): [AGENTS.md](AGENTS.md)

## 使い方

Rust (`rust-toolchain.toml` の版) と Bun (`web/.bun-version` の版) が要る。

```sh
git clone https://github.com/bannzai/tansu.git
cd tansu
make cli   # CLI を ~/.local/bin/tansu に置く
tansu --version
```

`tansu serve` と `tansu mcp` は未実装 (進捗は [documents/DIRECTION.md](documents/DIRECTION.md))。実装が進んだら、サーバーの起動手順と MCP サーバーの登録手順をここに書く。

## データの扱い

tansu は計測を持たず、ログインもサーバーを各 Mac の localhost で動かす間は持たず、外部への送信は利用者が選んだ Turso Cloud の DB への接続だけ (ローカルを選べば無い) という設計で作る (この時点では CLI の雛形だけで、以下は実装の方針)。データの置き場所は起動時に選ぶ (`TANSU_DB_URL`。`documents/PROJECT.md`「DB の置き場所」): 既定はこのマシンの SQLite ファイル (`TANSU_HOME` (既定 `~/.tansu`) の下の `tansu.sqlite`。実行中は付随ファイル `-wal` / `-shm` もできる) で、この時はマシンの外へ何も送らない。Turso Cloud の DB を指定した時は、データがその DB に保存され、外部への通信はその接続だけになる。どちらを選んでもこのマシンに書くファイルは利用記録 (`$TANSU_HOME/usage.jsonl`。画面を開いた日時と CLI の実行の種類だけ。データの内容は書かず、どこにも送らない)。サーバーは既定で `127.0.0.1` だけで待ち受ける。利用規約・プライバシーポリシーはこの記載で代える。
