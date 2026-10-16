---
feature: _root
verification: agent-browser
last_verified_commit: null
last_verified_at: null
---
# QA 全体ガイド

この QA.md は sample-sync の確認の進め方をまとめる。
各 feature の項目は features の下の QA.md にある。

## 対象環境

local (ブラウザで http://127.0.0.1:4100 を開く)

## 起動方法

```sh
bun run dev --port 4100
```

## 実行ナレッジ

### 2 台目の端末

- 発見日: 2026-09-02
- 事象: 2 台目のブラウザはシークレットウィンドウで開かないとログインが共有される
