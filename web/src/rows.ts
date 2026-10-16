import type { Database, NamedValue, Property, Row, RowValueInput } from "./api.ts";

/** 行の値を、編集欄が持つ形 (upsert に渡す値) にする。値が無ければ null。 */
export function inputValue(value: NamedValue | undefined): RowValueInput {
  return value === undefined ? null : value.value;
}

/** 編集欄の空の文字列を、値を消す null にする (空の text と値の無いプロパティを同じものとして扱う)。 */
function emptyToNull(value: RowValueInput): RowValueInput {
  return value === "" ? null : value;
}

/**
 * 編集欄の値 `draft` のうち、読み込んだ行 `row` の値から変わったものだけ (upsert に渡す形。空の文字列は値を消す null)。
 * 変えていないプロパティを送らないことで、読み込んだ後に CLI や別のタブが書いた値を上書きしない。
 */
export function changedValues(row: Row, draft: Record<string, RowValueInput>): Record<string, RowValueInput> {
  return Object.fromEntries(
    Object.entries(draft)
      .map(([name, value]) => [name, emptyToNull(value)] as const)
      .filter(([name, value]) => JSON.stringify(value) !== JSON.stringify(emptyToNull(inputValue(row.values[name])))),
  );
}

/**
 * 行 ID に使う文字と長さ。正は `crates/core/src/model.rs` の `ID_ALPHABET` / `ID_LENGTH` (nanoid の既定から `-` を外した
 * 63 文字・21 文字) で、画面が作る ID も CLI の ID と同じ形にするために写す。
 */
const ROW_ID_ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_";
const ROW_ID_LENGTH = 21;

/**
 * 「+ 新規」で足す行の ID。upsert は `id` か unique のプロパティの値が無い入力を拒否するため、画面で作って渡す。
 * `crypto.randomUUID` は secure context (https か localhost) でしか使えず、LAN の iPhone から http で開くと無いため、
 * どこでも使える `crypto.getRandomValues` で作る。
 */
export function newRowId(): string {
  // 256 は 63 で割り切れず剰余だと先頭の文字に偏るため、63 × 4 = 252 以上の値は捨てて引き直す
  const characters: string[] = [];
  while (characters.length < ROW_ID_LENGTH) {
    for (const byte of crypto.getRandomValues(new Uint8Array(ROW_ID_LENGTH))) {
      if (byte < ROW_ID_ALPHABET.length * 4 && characters.length < ROW_ID_LENGTH) {
        characters.push(ROW_ID_ALPHABET.charAt(byte % ROW_ID_ALPHABET.length));
      }
    }
  }
  return characters.join("");
}

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
