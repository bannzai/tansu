import { type Database, type DatabaseRows, fetchRows } from "./api.ts";
import { type RowTitles, rowTitle } from "./rows.ts";

/** `database` の relation のプロパティが指す相手のデータベースの定義と行。キーは相手のデータベースの ID。 */
export async function fetchRelationTargets(
  database: Database,
  databases: Database[],
): Promise<Map<string, DatabaseRows>> {
  const targetIds = new Set(
    database.properties.flatMap((property) =>
      property.kind.type === "relation" ? [property.kind.target_database_id] : [],
    ),
  );
  return new Map(
    await Promise.all(
      databases
        .filter((target) => targetIds.has(target.id))
        .map(async (target) => [target.id, await fetchRows(target.name)] as const),
    ),
  );
}

/** 相手のデータベースの行 ID → 行の名前。relation の値を名前で見せるために使う。 */
export function relationTitles(targets: ReadonlyMap<string, DatabaseRows>): RowTitles {
  return new Map(
    [...targets.values()].flatMap(({ database, rows }) =>
      rows.map((row) => [row.id, rowTitle(database, row)] as const),
    ),
  );
}

/** `error` (API の失敗・例外) の、画面に出す理由。 */
export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
