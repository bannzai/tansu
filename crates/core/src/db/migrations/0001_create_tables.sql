-- 物理テーブルは固定の 3 つで、利用者の表はこの 3 つのデータとして持つ (documents/data-model.md「SQLite / libSQL への持ち方」)。
-- database / row / values は SQLite のキーワードなので二重引用符で囲む。

CREATE TABLE "database" (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL
);

CREATE TABLE property (
  id TEXT PRIMARY KEY NOT NULL,
  database_id TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  unique_value INTEGER NOT NULL,
  position INTEGER NOT NULL,
  UNIQUE (database_id, name)
);

CREATE TABLE "row" (
  id TEXT PRIMARY KEY NOT NULL,
  database_id TEXT NOT NULL,
  body TEXT NOT NULL,
  "values" TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- 行の読み書きは必ずデータベースを絞ってから行う (query・unique の検査・relation の参照の削除)。
CREATE INDEX row_database_id ON "row" (database_id);
