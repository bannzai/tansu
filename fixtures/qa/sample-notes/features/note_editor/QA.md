---
feature: note_editor
verification: agent-browser, manual
last_verified_commit: a1b2c3d4e5f60718293a4b5c6d7e8f90ab12cd34
last_verified_at: 2026-10-07
---
# note_editor QA

## 関連リンク

- 仕様: 仕様なし QA
- 関連: https://example.com/sample-notes/issues/20

## 1. 編集

- [x] **本文を保存できる**: 本文を書いて保存すると一覧に反映される
  - 自動化: auto(e2e/flows/note_editor_save.yaml)
- [ ] **画像を貼れる**: 画像をドロップすると本文に埋め込まれる
  - 自動化: manual(ドロップ操作を E2E で再現できないため)
  - ❌ 失敗: ドロップ後に本文が空になる。再現手順: 画像を本文へドロップ → 保存。issue: https://example.com/sample-notes/issues/21
- [ ] **音声入力で書ける**: マイクのボタンを押すと音声が本文になる
  - 自動化: manual(マイクが要るため)
  - ⏭️ スキップ: シミュレータではマイクを使えない。実機で確認する
    - 起動条件: ローカル sim-boot。SIMSLIM_EXCEPT=default / SLIM_STATUS=applied
    - デーモン: `xcrun simctl spawn <UDID> launchctl list | grep -e com.example.audiod` の出力: 該当なし
    - 試した手順: マイクのボタンを押す → 権限のダイアログが出ない。設定アプリから権限を付ける → 項目が無い

#### 動作確認
<details>
<summary>動作確認エビデンス</summary>

### **本文を保存できる**: 本文を書いて保存すると一覧に反映される

<details><summary>動作確認スクショ</summary>

**確認日: 2026-10-07**
<img src="https://example.com/sample-notes/note_editor/save.png" width="320">

</details>

### **画像を貼れる**: 画像をドロップすると本文に埋め込まれる

<details><summary>動作確認スクショ</summary>

**確認日: 2026-10-07**
<img src="https://example.com/sample-notes/note_editor/image-drop-failed.png" width="320">

</details>

### **音声入力で書ける**: マイクのボタンを押すと音声が本文になる

<details><summary>動作確認スクショ</summary>

（未実行）

</details>

</details>
