-- 当て終えたマイグレーションの番号 (MIGRATIONS の 1 始まりの位置) を 1 行ずつ持つ。番号のマイグレーションより前に要るため、
-- 番号を付けずに毎回 IF NOT EXISTS で作る。PRAGMA user_version で持たないのは、Turso Cloud で user_version が読み取り専用のため
-- (https://docs.turso.tech/cloud/limitations「Read-only on Turso Cloud. Use a `_schema_version` table instead to track schema migrations.」)。

CREATE TABLE IF NOT EXISTS schema_migration (
  version INTEGER PRIMARY KEY NOT NULL
);
