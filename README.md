# tansu

ローカルで動く Notion 風のデータベース / CMS。自由にプロパティを定義した表を複数持ち、Web 画面で閲覧・絞り込み・入力し、AI agent からは CLI と MCP サーバーで読み書きする。最初のユースケースは、複数リポジトリの QA の項目と実行記録 (QA.md) の置き換え。

- 何を作るか・何で判定するか: [documents/DIRECTION.md](documents/DIRECTION.md)
- 要件と制約: [documents/PROJECT.md](documents/PROJECT.md)
- 開発の進め方 (検証は GitHub Actions で行う): [AGENTS.md](AGENTS.md)

## 使い方

(実装が進んだら、`git clone` からの起動手順・CLI・MCP サーバーの登録手順をここに書く)

## データの扱い

tansu はサーバー側の保存先・ログイン・計測・外部への送信を持たない。データはこのマシンの SQLite ファイル (既定 `~/.tansu/tansu.sqlite`) だけにあり、サーバーは既定で `127.0.0.1` だけで待ち受ける。利用規約・プライバシーポリシーはこの記載で代える。
