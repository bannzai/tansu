import type { NamedValue } from "./api.ts";
import { Link } from "./Link.tsx";
import { RESULT_LABELS } from "./qa.ts";
import { routePath } from "./route.ts";
import type { RowTitles } from "./rows.ts";
import { tagColor } from "./tags.ts";

/**
 * 1 つの値の表示。select / multi_select は選択肢の名前で決まる色の札、relation は相手の行へのリンク
 * (`targetDatabaseName` が分からなければ名前だけ)、url はリンク、date は日付にする。run の `result`
 * (ok / ng / later / skip) は結果の色の札にする。
 */
export function ValueView({
  propertyName,
  value,
  titles,
  targetDatabaseName,
}: {
  propertyName: string;
  value: NamedValue | undefined;
  titles: RowTitles;
  targetDatabaseName: string | undefined;
}) {
  if (value === undefined) {
    return null;
  }
  switch (value.type) {
    case "select":
      if (value.value === null) {
        return null;
      }
      return propertyName === "result" && RESULT_LABELS[value.value] !== undefined ? (
        <span className={`result result-${value.value}`}>{RESULT_LABELS[value.value]}</span>
      ) : (
        <span className={`tag tag-color-${tagColor(value.value)}`}>{value.value}</span>
      );
    case "multi_select":
      return (
        <span className="tags">
          {value.value.map((name) => (
            <span key={name} className={`tag tag-color-${tagColor(name)}`}>
              {name}
            </span>
          ))}
        </span>
      );
    case "relation":
      return (
        <span className="tags">
          {value.value.map((rowId) =>
            targetDatabaseName === undefined ? (
              <span key={rowId}>{titles.get(rowId) ?? rowId}</span>
            ) : (
              <Link key={rowId} to={routePath({ page: "row", database: targetDatabaseName, rowId })}>
                {titles.get(rowId) ?? rowId}
              </Link>
            ),
          )}
        </span>
      );
    case "url":
      // `javascript:` などの URL をリンクにすると、押した時にこの画面のオリジンで実行されるため、http(s) だけをリンクにする
      return /^https?:\/\//i.test(value.value) ? (
        <a href={value.value} target="_blank" rel="noreferrer">
          {value.value}
        </a>
      ) : (
        <span>{value.value}</span>
      );
    case "checkbox":
      return <span>{value.value ? "✓" : "—"}</span>;
    case "number":
      return <span>{value.value}</span>;
    case "date":
      return <time dateTime={value.value}>{dateText(value.value)}</time>;
    case "text":
      return <span>{value.value}</span>;
  }
}

/**
 * date の値 (日付か時差付き ISO 8601) の表示。日付だけの値はそのまま、時刻を持つ値はこの端末の時刻で
 * 年月日と時分にする。読めない値はそのまま出す。
 */
function dateText(value: string): string {
  if (/^\d{4}-\d{2}-\d{2}$/.test(value) || Number.isNaN(Date.parse(value))) {
    return value;
  }
  return new Date(value).toLocaleString("ja-JP", { dateStyle: "medium", timeStyle: "short" });
}
