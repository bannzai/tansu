import type { Database, NamedValue, Property, Row } from "./api.ts";

/** relation の行 ID → 相手の行の名前 ([`rowTitle`])。 */
export type RowTitles = ReadonlyMap<string, string>;

/** 行の名前として見せるプロパティ。`name` という名前の text、無ければ最初の text。どちらも無ければ undefined。 */
export function titleProperty(database: Database): Property | undefined {
  return (
    database.properties.find((property) => property.name === "name" && property.kind.type === "text") ??
    database.properties.find((property) => property.kind.type === "text")
  );
}

/** 行の名前。[`titleProperty`] の値で、無ければ行 ID (名前を持たない run の行もどの行かを示せるように)。 */
export function rowTitle(database: Database, row: Row): string {
  const property = titleProperty(database);
  const value = property === undefined ? undefined : row.values[property.name];
  return value?.type === "text" && value.value !== "" ? value.value : row.id;
}

/** 値を 1 行の文字列にする。表のセル・並べ替え・テキストの絞り込みが使う。relation は相手の行の名前にする。 */
export function valueText(value: NamedValue | undefined, titles: RowTitles): string {
  if (value === undefined) {
    return "";
  }
  switch (value.type) {
    case "text":
    case "url":
    case "date":
      return value.value;
    case "number":
      return String(value.value);
    case "checkbox":
      return value.value ? "✓" : "";
    case "select":
      return value.value ?? "";
    case "multi_select":
      return value.value.join(", ");
    case "relation":
      return value.value.map((rowId) => titles.get(rowId) ?? rowId).join(", ");
  }
}

/** 表の絞り込みの条件。空の `value` は絞り込まない。 */
export type RowFilter = {
  /** 絞り込むプロパティの名前。 */
  property: string;
  /**
   * select はその選択肢の名前と一致、multi_select はその名前を含む、relation はその行 ID を含む行に一致する。
   * それ以外の型は [`valueText`] の文字列に部分一致 (大文字と小文字を区別しない) する行。
   */
  value: string;
};

/** `rows` のうち、`filters` のすべてに一致する行 (並びは変えない)。 */
export function filterRows(rows: Row[], filters: RowFilter[], titles: RowTitles): Row[] {
  return rows.filter((row) =>
    filters.every((filter) => filter.value === "" || matches(row.values[filter.property], filter.value, titles)),
  );
}

/** 値 `value` が絞り込みの値 `expected` に一致するか。規則は [`RowFilter`] の `value`。 */
function matches(value: NamedValue | undefined, expected: string, titles: RowTitles): boolean {
  if (value === undefined) {
    return false;
  }
  switch (value.type) {
    case "select":
      return value.value === expected;
    case "multi_select":
    case "relation":
      return value.value.includes(expected);
    default:
      return valueText(value, titles).toLowerCase().includes(expected.toLowerCase());
  }
}

/** 表の並べ替え。 */
export type RowSort = {
  /** 並べ替えるプロパティの名前。 */
  property: string;
  /** 昇順か降順か。 */
  direction: "asc" | "desc";
};

/**
 * `rows` を `sort` で並べ替えた新しい配列。number は数、date は時刻、それ以外は [`valueText`] の文字列で比べる。
 * 値の無い行は昇順でも降順でも最後に置き (`tansu query --sort` と同じ)、同じ値の行は元の順を保つ。
 */
export function sortRows(rows: Row[], sort: RowSort, titles: RowTitles): Row[] {
  const direction = sort.direction === "asc" ? 1 : -1;
  return [...rows].sort((left, right) => {
    const leftValue = left.values[sort.property];
    const rightValue = right.values[sort.property];
    if (leftValue === undefined || rightValue === undefined) {
      return Number(leftValue === undefined) - Number(rightValue === undefined);
    }
    return direction * compareValues(leftValue, rightValue, titles);
  });
}

/** 2 つの値の大小。規則は [`sortRows`]。 */
function compareValues(left: NamedValue, right: NamedValue, titles: RowTitles): number {
  if (left.type === "number" && right.type === "number") {
    return left.value - right.value;
  }
  if (left.type === "date" && right.type === "date") {
    return Date.parse(left.value) - Date.parse(right.value);
  }
  return valueText(left, titles).localeCompare(valueText(right, titles), "ja");
}
