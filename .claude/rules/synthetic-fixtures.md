---
paths:
  - "fixtures/**"
  - "e2e/**"
  - "**/*.test.ts"
  - "documents/design/**"
---

# fixture とスクリーンショットは手書きの合成 QA.md から作る

bannzai の各リポジトリの QA.md は、private リポジトリの機能の項目・実行記録・エビデンス画像の URL を含み、このリポジトリは public である。そのため、commit するもの・PR に添付するもの・CI の artifact に上げるものは、すべて手書きの合成 QA.md と合成のデータから作る (`documents/PROJECT.md`「制約」)。

- fixture の QA.md は、castle の `setup-qa/references/qa-md-format.md` の構文 (frontmatter・セクション・項目・自動化ステータス・⏭️ / ❌ の記録・エビデンスブロック) をすべて含むように手書きする。プロジェクト名・feature 名・項目の文面は架空のものにし、画像の URL は `https://example.com/...` にする
- スクリーンショットは、CI で `fixtures/` を取り込ませたアプリから撮る。本物の QA.md を取り込んだ画面のスクリーンショットは commit も添付もしない
- パーサの不具合を再現するために本物の QA.md の行が要る時は、構造 (記法) だけを残し、すべての文面を書き換えてから fixture にする
- テストは `TANSU_DB` を一時ファイルにして fixture を取り込む。実際のホームディレクトリの DB は読まない
- 158 ファイルの本物の QA.md での欠落ゼロの検証は、このリポジトリの CI では行わず、castle の import → 削除の skill (https://github.com/bannzai/castle/issues/1359 ) が bannzai のマシンで行う
