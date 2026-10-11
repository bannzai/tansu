---
feature: _root
verification: agent-browser
last_verified_commit: null
last_verified_at: null
---
# QA 全体ガイド

## 対象環境

local (ブラウザで http://127.0.0.1:4000 を開く)

## 起動方法

`bun run dev`

## ログイン方法

テストアカウントの認証情報は環境変数 `SAMPLE_NOTES_TEST_PASSWORD` を参照する

## 動作確認手段

agent-browser skill で操作する

## 実行ナレッジ

### 初回起動が遅い

- 発見日: 2026-10-01
- 事象: 初回の `bun run dev` は依存の解決で 30 秒かかる
- 対処: 2 回目以降は数秒で起動する

## 横断確認項目

## 1. 起動

- [x] **トップページが開く**: `/` を開くとノートの一覧が表示される
- [ ] **ログインできる**: テストアカウントでログインするとヘッダーに名前が出る
  - ⏭️ スキップ: テストアカウントの発行待ち

#### 動作確認
<details>
<summary>動作確認エビデンス</summary>

### **トップページが開く**: `/` を開くとノートの一覧が表示される

<details><summary>動作確認スクショ</summary>

**確認日: 2026-10-05**
<img src="https://example.com/sample-notes/root/top.png" width="320">

</details>

### **ログインできる**: テストアカウントでログインするとヘッダーに名前が出る

<details><summary>動作確認スクショ</summary>

（未実行）

</details>

</details>

## 機能別 QA.md

- [features/note_list/QA.md](features/note_list/QA.md)
- [features/note_editor/QA.md](features/note_editor/QA.md)
- [features/settings/QA.md](features/settings/QA.md)

## QA 対象外

| feature | 理由 |
| --- | --- |
| `shared` / `theme` | UI を持たない共通コードで、各 feature の項目で確認する |
