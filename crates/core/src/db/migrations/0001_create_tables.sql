-- 物理テーブルは固定の 3 つで、利用者の表はこの 3 つのデータとして持つ (documents/data-model.md「SQLite / libSQL への持ち方」)。
-- values は SQLite の予約語で引用符が必須。database / row は引用符無しでも識別子に使えるが揃えて囲む。

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

CREATE INDEX row_database_id ON "row"(database_id);
