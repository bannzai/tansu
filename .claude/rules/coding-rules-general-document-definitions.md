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
# 定義にはdocumentコメントを書く

すべてのプログラミング言語に共通するルール。

## ルール

- 構造の定義（class, struct, enum, property）をした場合はdocumentコメントを書く
- 関数の定義をした場合はdocumentコメントを書く
- documentコメントに書くのは、その定義が何を表しているか (構造) と、何を受け取り何を返すか・副作用・前提 (関数) の契約。処理の手順 (本体を読めばわかる「何をしているか」) は書かない。`coding-rules-general-single-source-info.md` の「ロジックについて何をしているかは書かない」は本体のコメントと document コメントの中身に効き、document コメントを書くこと自体はこちらに従う
- documentコメントの言語は `~/.claude/CLAUDE.md`「基本ルール」に従う（OSS として作成するプロジェクトは英語、それ以外は日本語。下の例は日本語の場合）

## 悪い例

```go
type User struct {
	ID   int
	Name string
}

func CreateUser(name string) *User {
	return &User{Name: name}
}
```

```typescript
interface User {
  id: number;
  name: string;
}

function createUser(name: string): User {
  return { id: 0, name };
}
```

## 良い例

```go
// User はアプリケーションのユーザーを表す。
type User struct {
	// ID はユーザーの一意な識別子。
	ID int
	// Name はユーザーの表示名。
	Name string
}

// CreateUser は指定された名前で新しいUserを作成する。
func CreateUser(name string) *User {
	return &User{Name: name}
}
```

```typescript
/** アプリケーションのユーザーを表す。 */
interface User {
  /** ユーザーの一意な識別子。 */
  id: number;
  /** ユーザーの表示名。 */
  name: string;
}

/** 指定された名前で新しいUserを作成する。 */
function createUser(name: string): User {
  return { id: 0, name };
}
```
