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
# 値の帰属を検証せずに表示・流用しない

複数の主体 (アカウント・ユーザー・テナント・環境) で共有されうる保存先から読んだ値や、取得を任せた外部のコマンド・SDK が返した値を、「どの主体の値か」を確認せずに表示・利用しない。

bannzai/yoyu (Claude Code / Codex の利用枠を表示するメニューバーアプリ) で、設定に並べた claude の 6 アカウントに、使用率もリセット時刻も同一の値を表示した実例がある。yoyu はアカウントごとに `claude -p '/usage'` を実行し、claude は毎回 `GET /api/oauth/usage` でサーバーから使用率を取得していた。ところが使用率 API が HTTP 200 のままエラー本文を返すことがあり、その時 claude はエラーにせず、以前に保存した値を `/usage` に表示した。設定ディレクトリを複数アカウントで共有する構成では、その値は最後に取得に成功した別アカウントのものだった (claude 2.1.273 の debug ログで確認。失敗するアカウントは実行のたびに変わり、4 時間空けた後は 18 回の実行すべてが成功してアカウントごとに別の値が返った)。依存先の CLI が、取得に失敗した時に黙って別の主体の値へ切り替えていたことになる。出力には成功と失敗の区別が無く、呼び出し側は出力だけでは帰属を確認できなかった。値が空でもエラーでもなく、もっともらしい数字だったため、ユーザーが「全部リセットする日が一緒でおかしい」と気づくまで表示され続けた。

## ルール

- 複数の主体 (アカウント・ユーザー・テナント・環境) で共有されうる保存先 (キャッシュ・設定ファイル・共有ディレクトリ) から読んだ値は、「どの主体の値か」を示す識別子が現在の主体と一致することを確認してから使う。識別子が無い・照合できない場合は、その値を表示・利用しない
- 「取得に失敗した」だけでなく「取得できたが自分のものか確認できない」も、値を出してはいけない状態として扱う。空表示・エラー表示・その行を出さない、のいずれかにする
- 値の取得を外部のコマンド・SDK に任せている時は、その出力が「今回取得した値」か「失敗時に代わりに出された値」かを呼び出し側で見分けられるかを確認する。見分ける手段 (exit code・エラー出力・debug ログ等) が無い、またはその手段で今回の取得成功を確認できなかった値は表示しない
- 主体ごとに値を出す UI (アカウント別の一覧など) では、表示の単位と実際の取得単位が一致していることを検証してから並べる。検証できないなら主体ごとに分けて表示しない (1 行にまとめる・表示自体をやめる)
- もっともらしい値が出ていることを動作確認の根拠にしない。複数の主体で同じ値が並ぶ・区別がつかない出力は、正常ではなく検証不足の兆候として扱う

## 悪い例

キャッシュが持つ `accountUuid` を照合せず、引数で渡されたアカウントの使用率として表示している。共有の保存先を読む限り、どのアカウントを渡しても同じ値が返る。

```swift
func usageText(for account: Account) -> String? {
    guard let cache = loadUsageCache(configDirectory: sharedConfigDirectory) else {
        return nil
    }
    return "\(cache.utilization.sevenDay.utilization)%"
}
```

取得は共有パス 1 つに対してしか行えないのに、アカウントの一覧をループしてアカウントごとの行として並べている。表示の単位 (アカウント別) と取得の単位 (共有パス 1 つ) が一致していない。

```typescript
const rows = accounts.map((account) => ({
  name: account.name,
  usage: readUsageCache(SHARED_CONFIG_PATH).utilization,
}));
```

## 良い例

アカウントごとの保存先から読み、キャッシュの `accountUuid` がそのアカウントと一致した時だけ表示する。一致しなければ別のアカウントの値なので行を出さない。

```swift
func usageText(for account: Account) -> String? {
    guard let cache = loadUsageCache(configDirectory: account.configDirectory),
          cache.accountUuid == account.uuid else {
        return nil
    }
    return "\(cache.utilization.sevenDay.utilization)%"
}
```

取得単位をアカウントごとに分けられないなら、アカウント別の行に分けない。識別子が指すアカウントの 1 行だけを出し、識別子から主体を特定できなければ何も出さない。

```typescript
const cache = readUsageCache(SHARED_CONFIG_PATH);
const owner = accounts.find((account) => account.uuid === cache.accountUuid);
const rows = owner ? [{ name: owner.name, usage: cache.utilization }] : [];
```

外部のコマンドに取得を任せ、その出力だけでは今回取得した値か失敗時の代替値かを見分けられない時は、別の経路 (ここではコマンドの debug ログ) で取得の成否を確かめてから値を採用する。成功の記録があり、かつエラーの記録が無い時だけ通す。どちらの記録も無い (ログを読めない・形式が変わった) 時も通さない。

```swift
func verifyUsageFetch(debugLogText: String) throws {
    if debugLogText.contains("Usage fetch returned a fieldless or non-object body") {
        throw UsageClientError.usageFetchFailed
    }
    if !debugLogText.contains("fetchUtilization: 200") {
        throw UsageClientError.usageFetchUnverified
    }
}
```

## 既存ルールとの関係

- `coding-rules-general-no-default-for-external-source-of-truth.md` は、外部が正を持つ値にそれらしい固定値 (`?? "¥800"` のようなデフォルト) を置かないルール。あちらの対象は「取得できなかった時に自分で作った値」、本ルールの対象は「取得できたが別の主体に帰属する実データ」で、実データの方が値として自然に見えるぶん気づきにくい。依存先のコマンド・SDK が取得に失敗した時に別の主体の実データを代わりに出す場合は、フォールバックを書いたのが自分のコードではなく依存先で、出てくるのも固定値ではなく実データなので、本ルールの対象にする。実際と異なる値をユーザーに見せず、表示しない・エラーにする状態へ倒す点は同じ
- `coding-rules-general-default-value-rationale.md` は、クライアントが正を持ちデフォルト値を定義してよい場合に選定根拠をコメントで書くルール。本ルールが求める「識別子が一致しない時は値を出さない」は、別の主体の値をデフォルト代わりに使わないことを意味し、根拠を書けば流用してよいという緩和ではない
- `firebase-environment-check.md` は、デプロイ・診断の前に対象環境と実体の更新時刻を確認するルール。あちらは「同じ主体の古い実体」を疑う規律で、本ルールは「別の主体の実体」を疑う規律。対象が異なる

## 出典

- 起票元: https://github.com/bannzai/castle/issues/1097
- 実例の事実関係の訂正と、外部のコマンド・SDK の出力に関する規律の追加: https://github.com/bannzai/castle/issues/1116 (起票元の背景説明は原因を「共有キャッシュの読み取り」と取り違えていた。調査の記録と実測は https://github.com/bannzai/yoyu/pull/42 )
- 発生元: bannzai/yoyu (Claude Code / Codex の利用枠を表示するメニューバーアプリ)
