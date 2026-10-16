---
feature: sync
verification: agent-browser
last_verified_commit: 0123456789abcdef0123456789abcdef01234567
last_verified_at: 2026-09-20
---
# sync QA

前提: テスト用のアカウントを 2 つ用意し、両方の端末でログインしておく。

**2026-09-01 で判明した制約**: オフラインの間の編集は 50 件までしか送られない。

## 1. 送信

- [x] **編集が別の端末に届く**: 端末 A で編集すると端末 B に 5 秒以内に反映される
  - 自動化: auto(e2e/flows/sync_send.yaml)
  - 補足: 端末 B は画面を開いたままにする
- [ ] **オフラインの編集を送る**: 通信が戻るとオフラインの間の編集が送られる
  - 自動化: unit（sync_queue_test.rs）
  - ❌ 失敗: 51 件目以降が送られない

この節の項目は、端末 A と B を同じアカウントでそろえてから確認する。

## 2. 競合

- [x] **競合を表示する**: 同じノートを 2 台で編集すると競合の印が出る
  - 自動化: manual(2 台の操作のタイミングを E2E で再現できないため)
- [x] **競合を表示する**: 競合の印を押すと両方の版を並べて見られる
  - 自動化: todo
  ```sh
  sample-sync conflict --note demo
  ```

## 同期の方式の変更に関する注記

2026-09-10 に差分の送り方を変えた。それより前の確認日の画像は旧方式の画面。

#### 動作確認
<details>
<summary>動作確認エビデンス</summary>

### **編集が別の端末に届く**: 端末 A で編集すると端末 B に 5 秒以内に反映される

<details><summary>動作確認スクショ</summary>

**確認日: 2026-09-01**
<img src="https://example.com/lossless-notes/sync/send-1.png" width="320">
端末 B の一覧の先頭に出た

**再確認日: 2026-09-12**
<img src="https://example.com/lossless-notes/sync/send-2a.png" width="320"> <img src="https://example.com/lossless-notes/sync/send-2b.png" width="320">
差分の送り方を変えた後も届いた

**再確認日: 2026-09-20（iPad は未確認）**
<img src="https://example.com/lossless-notes/sync/send-3.png" width="320">
```text
received 1 change in 2.1s
```

</details>

### **オフラインの編集を送る**: 通信が戻るとオフラインの間の編集が送られる

<details><summary>動作確認スクショ</summary>

前回の確認日から送信の上限が変わっていない
**確認日: 2026-09-20**
<img src="https://example.com/lossless-notes/sync/offline.png" width="320">

</details>

### **競合を表示する**: 同じノートを 2 台で編集すると競合の印が出る

<details><summary>動作確認スクショ (端末 A と B)</summary>

**確認日: 2026-09-15**
<img src="https://example.com/lossless-notes/sync/conflict-mark.png" width="320"> 競合の印はノートの右上に出た

</details>

### **競合を表示する**: 競合の印を押すと両方の版を並べて見られる

<details><summary>動作確認スクショ</summary>

**確認日: 2026-09-16**
<img src="https://example.com/lossless-notes/sync/conflict-compare.png" width="320">

</details>

</details>
