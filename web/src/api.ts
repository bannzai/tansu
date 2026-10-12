// HTTP API (crates/server/src/lib.rs) の呼び出しと、その JSON の型。
// 型の正は Rust の serde の表現 (crates/core/src/model.rs と named_json.rs、qa/input.rs)。生成の仕組みが無いため、
// 画面が読むキーだけを写す。

/** select / multi_select の選択肢 (model.rs の `SelectOption`)。 */
export type SelectOption = {
  /** 選択肢の ID。 */
  id: string;
  /** 利用者に見せる名前。行の値はこの名前で読み書きする。 */
  name: string;
};

/** プロパティの型と設定 (model.rs の `PropertyKind`)。 */
export type PropertyKind =
  | { type: "text" | "number" | "checkbox" | "url" | "date" }
  | { type: "select" | "multi_select"; options: SelectOption[] }
  | { type: "relation"; target_database_id: string };

/** 表の列の定義 (model.rs の `Property`)。 */
export type Property = {
  /** プロパティの ID。 */
  id: string;
  /** データベースの中で一意な名前。行の `values` のキー。 */
  name: string;
  /** 型とその設定。 */
  kind: PropertyKind;
};

/** 利用者が定義した表 (model.rs の `Database`)。 */
export type Database = {
  /** データベースの ID。relation の `target_database_id` が指す。 */
  id: string;
  /** すべてのデータベースの中で一意な名前。URL と API のパスに使う。 */
  name: string;
  /** 列の定義 (並び順)。 */
  properties: Property[];
};

/** 行の 1 つの値 (named_json.rs の形。select / multi_select は選択肢の名前、relation は相手の行 ID)。 */
export type NamedValue =
  | { type: "text" | "url" | "date"; value: string }
  | { type: "number"; value: number }
  | { type: "checkbox"; value: boolean }
  | { type: "select"; value: string | null }
  | { type: "multi_select" | "relation"; value: string[] };

/** 表の 1 行 (named_json.rs の `row_to_json`)。 */
export type Row = {
  /** 行 ID。 */
  id: string;
  /** markdown の本文。 */
  body: string;
  /** プロパティ名 → 値。値の無いプロパティはキーが無い。 */
  values: Partial<Record<string, NamedValue>>;
  /** 最後に書いた時刻 (時差付き ISO 8601)。 */
  updated_at: string;
};

/** `GET /api/databases/{database}/rows` の応答 (export.rs の `to_json`)。 */
export type DatabaseRows = {
  /** データベースの定義。 */
  database: Database;
  /** すべての行 (追加した順)。 */
  rows: Row[];
};

/** `GET /api/qa/session` の応答 (qa/input.rs の `InputSession`)。 */
export type QaSession = {
  /** project の `owner/repo`。 */
  repo: string;
  /** feature 名。 */
  feature: string;
  /** project の checkout の HEAD の full SHA。null なら記録できない。 */
  head_commit: string | null;
  /** feature の item (取り込んだ順) と、その最後の run。 */
  items: { item: Row; latest_run: Row | null }[];
};

/** upsert に渡す値。null はその値を消す。 */
export type RowValueInput = string | number | boolean | string[] | null;

/** API のエラー本文 `{"error": <理由>}` の理由。読めなければ状態コードを返す。 */
async function errorMessage(response: Response): Promise<string> {
  const body: unknown = await response.json().catch(() => null);
  if (typeof body === "object" && body !== null && "error" in body && typeof body.error === "string") {
    return body.error;
  }
  return `HTTP ${response.status}`;
}

/** `path` を取得して JSON を返す。失敗した時は API のエラーの理由で reject する。 */
async function getJson<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) {
    throw new Error(await errorMessage(response));
  }
  return (await response.json()) as T;
}

/**
 * `path` に JSON を POST する。書き込みの API は `Content-Type: application/json` だけを受け付ける (CSRF 対策。
 * documents/PROJECT.md「制約」)。応答が本文を持たない (204) 時は undefined を返す。
 */
async function postJson<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(await errorMessage(response));
  }
  return (response.status === 204 ? undefined : await response.json()) as T;
}

/** API のパスに入れるデータベース名。 */
function databasePath(databaseName: string): string {
  return `/api/databases/${encodeURIComponent(databaseName)}`;
}

/** すべてのデータベースの定義 (名前順)。 */
export function fetchDatabases(): Promise<Database[]> {
  return getJson("/api/databases");
}

/** データベース `databaseName` の定義とすべての行。 */
export function fetchRows(databaseName: string): Promise<DatabaseRows> {
  return getJson(`${databasePath(databaseName)}/rows`);
}

/** 行 `row.id` に値を書き、書いた行を返す (`tansu upsert`)。`values` に無いプロパティと、`body` を省いた時の本文は変えない。 */
export function upsertRow(
  databaseName: string,
  row: { id: string; body?: string; values: Record<string, RowValueInput> },
): Promise<Row> {
  return postJson(`${databasePath(databaseName)}/rows`, row);
}

/** `tansu db schema --add` と同じ指定でプロパティ・選択肢を足し、変えた後の定義を返す。 */
export function addProperties(databaseName: string, add: string[]): Promise<Database> {
  return postJson(`${databasePath(databaseName)}/schema`, { add });
}

/** project `repo` の feature `feature` の QA の入力ビューの内容。 */
export function fetchQaSession(repo: string, feature: string): Promise<QaSession> {
  return getJson(`/api/qa/session?${new URLSearchParams({ repo, feature })}`);
}

/** QA の結果を run に書く (`tansu qa record --item`)。同じ `run_id` の run は 1 行にする。 */
export function recordQa(input: {
  item_id: string;
  result: "ok" | "ng" | "later";
  commit: string;
  note: string | null;
  env: string | null;
  run_id: string;
}): Promise<Row> {
  return postJson("/api/qa/record", input);
}

/** 画面を読み込んだことを利用記録 (usage.jsonl) に書く。documents/DIRECTION.md「判定基準」の計測元。 */
export function recordWebOpen(): Promise<void> {
  return postJson("/api/usage/web-open", {});
}
