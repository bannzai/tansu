---
paths:
  - "**/*.go"
  - "**/*.ts"
  - "**/*.tsx"
  - "**/*.js"
  - "**/*.jsx"
  - "**/*.py"
  - "**/*.swift"
  - "**/*.kt"
  - "**/*.rb"
  - "**/*.rs"
  - "**/*.java"
  - "**/*.c"
  - "**/*.cpp"
  - "**/*.h"
  - "**/*.dart"
---
# 外部が正を持つ値にクライアントのデフォルト値を置かない

ストア、サーバー、外部 API が決定し、クライアントは取得して使うだけの値には、クライアント側でデフォルト値や仮の固定値を定義しない。

## ルール

- 課金価格、通貨、無料トライアル期間、導入価格、サーバーが返す表示文言や URL など、外部が正を持つ値を、それらしい固定値へフォールバックしない
- 値を取得できない状態はエラーまたは利用不可として扱う。値が必要なプランや機能を表示しない、エラーを表示する、再取得の導線を出すなど、実際と異なる値をユーザーに見せない状態へ遷移させる
- 値を表示するのは、正を持つ外部から取得できた時だけにする。取得失敗を Optional の `??`、空文字、固定 URL、固定価格・期間などで隠さない
- Preview、テスト、または SDK 未設定を明示した開発用状態で見本値が必要な場合は、実運用の取得失敗時に選ばれない fixture や mock に隔離する。本番の表示経路には流用しない

## 悪い例

外部から価格を取得できない時に、日本円の固定価格を表示している。ストアフロントが米国なら、実際の請求額と異なる通貨と金額を見せることになる。

```swift
private var monthlyPriceText: String {
    offering?.monthly?.storeProduct.localizedPriceString ?? "¥800"
}
```

無料トライアルを取得できない時に、ストア設定とは無関係な期間を表示している。

```typescript
const trialDays = product.introductoryOffer?.trialDays ?? 7;
```

## 良い例

取得できた package の価格だけを表示し、取得できないプランは表示しない。

```swift
if let package = offering?.monthly {
    MonthlyPlanCard(price: package.storeProduct.localizedPriceString)
}
```

offering を取得できても購入可能な package が無ければ成功扱いにせず、エラーと再取得の導線を表示する。

```swift
guard let offering, !offering.availablePackages.isEmpty else {
    state = .failed
    return
}
state = .loaded(offering)
```

無料トライアルの文言は、取得できた offer の期間からだけ組み立てる。offer が無い時はトライアル文言を表示しない。

```swift
guard let period = package.storeProduct.introductoryDiscount?.subscriptionPeriod else {
    return nil
}
return trialDescription(period: period)
```

## 既存リポジトリの点検

このルールを適用するリポジトリでは、正を持つ外部から値を取得する実装をリポジトリ全体で洗い出し、固定値へのフォールバックが残っていないことを確認する。課金価格を表示する Swift コードでは、まず次の検索で取得箇所と固定価格の候補を探す。

```sh
rg -n 'localizedPriceString|localizedPrice' -g '*.swift'
rg -n '\?\?\s*"[^"]*(¥|\$|円|USD)|Text\(verbatim:\s*"[^"]*(¥|\$|円|USD)' -g '*.swift'
```

無料トライアル期間・導入価格は `introductory`、`trial`、`freeTrial`、`無料`、`トライアル`、サーバー由来の表示文言・URL はレスポンスフィールドの参照箇所を起点に検索する。見つけた固定値が Preview・テスト用 fixture ではなく実運用の取得失敗時に使われるなら、値を表示しないか、エラー・再取得の状態へ変更する。

## 既存ルールとの関係

- `coding-rules-general-default-value-rationale.md` は、クライアントが正を持ちデフォルト値を定義してよい場合に、その値の選定根拠を書くルール。本ルールはその前段でデフォルト値を定義してよいかを判断する。外部が正を持つ値には、コメントで根拠を書いてもデフォルト値を定義しない
- `coding-rules-general-verify-value-ownership.md` は、複数の主体で共有されうる保存先から読んだ値を、帰属を示す識別子と照合せずに表示・流用しないルール。本ルールが扱うのは「取得できなかった時に自分で作った固定値」、あちらが扱うのは「取得できたが別の主体に帰属する実データ」で、どちらも実際と異なる値をユーザーに見せない状態へ倒す
- `coding-rules-general-single-source-info.md` は同じ情報を複数箇所に書かない一般原則。本ルールは、外部が正を持つ値を取得できない時の表示とエラー処理を具体化する
- `~/.claude/documents/rules/iap-product-identifier-naming.md` はストアへ登録する商品識別子の命名を扱う。識別子に価格を含める規約は、ストアから取得したローカライズ済み価格の表示や取得失敗時のフォールバックとは対象が異なる

## 出典

- 起票元: https://github.com/bannzai/castle/issues/741
- 発生元: https://github.com/bannzai/igen/issues/59
