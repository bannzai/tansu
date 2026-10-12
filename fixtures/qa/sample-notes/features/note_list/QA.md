---
feature: note_list
verification: agent-browser
last_verified_commit: 3f2a9c1e5b7d4a6f8c0e2b4d6f8a1c3e5b7d9f0a
last_verified_at: 2026-10-06
---
# note_list QA

## 関連リンク

- 仕様: https://example.com/sample-notes/issues/12
- 関連: https://example.com/sample-notes/pull/34

## 仕様チェックリスト

| ID | 期待挙動 | 対応項目 |
|----|---------|---------|
| S1 | ノートは更新日時の新しい順に並ぶ | 一覧の並び |
| S2 | 検索語を含むノートだけが表示される | 検索で絞り込める |
| S3 | ノートが 0 件なら案内文を表示する | — |
| S4 | ピン留めしたノートは先頭に固定される | ピン留めの固定 |
| S5 | 削除したノートはゴミ箱に移る | ゴミ箱へ移動 |

## 1. 表示

- [x] **一覧の並び**: ノートが更新日時の新しい順に並ぶ
  - 自動化: auto（e2e/flows/note_list_order.yaml）
- [x] **検索で絞り込める**: 検索語を入れると、含むノートだけが残る
  - 自動化: manual（検索語の入力タイミングで結果が変わるため目視で確認する）
- [ ] **空の案内**: ノートが 0 件なら「ノートがありません」と表示される
  - 自動化: todo
- [ ] **ピン留めの固定**: ピン留めしたノートが先頭に固定される
  - 自動化: todo
  - 優先度: 高

#### 動作確認
<details>
<summary>動作確認エビデンス</summary>

### **一覧の並び**: ノートが更新日時の新しい順に並ぶ

<details><summary>動作確認スクショ</summary>

**確認日: 2026-10-06**
<img src="https://example.com/sample-notes/note_list/order.png" width="320">

</details>

### **検索で絞り込める**: 検索語を入れると、含むノートだけが残る

<details><summary>動作確認スクショ</summary>

**確認日: 2026-10-06**
<img src="https://example.com/sample-notes/note_list/search.png" width="320">

</details>

### **空の案内**: ノートが 0 件なら「ノートがありません」と表示される

<details><summary>動作確認スクショ</summary>

（未実行）

</details>

### **ピン留めの固定**: ピン留めしたノートが先頭に固定される

<details><summary>動作確認スクショ</summary>

（未実行）

</details>

</details>

---

## 2. 操作

- [ ] **スワイプで削除**: 一覧の行を左にスワイプすると削除ボタンが出る
  - 自動化: auto(e2e/flows/note_list_swipe.yaml)
- [ ] 太字の無い項目: この行は項目として読めない

#### 動作確認
<details>
<summary>動作確認エビデンス</summary>

### **スワイプで削除**: 一覧の行を左にスワイプすると削除ボタンが出る

<details><summary>動作確認スクショ</summary>

（未実行）

</details>

</details>
