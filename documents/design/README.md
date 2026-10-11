# documents/design

関門 2 (デザイン) で agent が Claude Design の canvas に作ったモックと、bannzai から受領したデザインの置き場 (create-new-app skill Phase 7)。canvas の URL はアカウントの切り替えで開けなくなることがあるため、canvas に公開したファイル (`canvas.json` と artboard ごとの `*.dc.html`) と、単独で開ける `index.html` をここに保存する。

| ディレクトリ | 内容 |
| --- | --- |
| `mock-2026-10-11/` | 関門 2 のモック 4 画面 (表ビュー・QA の入力ビュー・行の詳細ページ・iPhone の QA の入力ビュー)。canvas: https://claude.ai/artifact/7Xk3pGXaAqsu7TPZPwYMYX |

`*.dc.html` は canvas の実行環境 (`./support.js`) を前提にしていて単体では表示できない。見る時は `index.html` をブラウザで開く。`index.html` は `*.dc.html` の繰り返しと状態を手で展開した複製で、モックは関門 2 の判断材料として保存した時点のものを残し、以後は更新しない (更新するなら canvas を直して両方を保存し直す)。このディレクトリは canvas の出力をそのまま保存するため、Biome の lint と整形検査の対象から外している (`biome.json` の `!documents/design`)。
