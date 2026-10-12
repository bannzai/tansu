import type { NamedValue } from "./api.ts";
import { Link } from "./Link.tsx";
import { RESULT_LABELS } from "./qa.ts";
import { routePath } from "./route.ts";
import type { RowTitles } from "./rows.ts";

/**
 * 1 つの値の表示。select / multi_select は札、relation は相手の行へのリンク (`targetDatabaseName` が分からなければ名前だけ)、
 * url はリンクにする。run の `result` (ok / ng / later / skip) は結果の色の札にする。
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
        <span className="tag">{value.value}</span>
      );
    case "multi_select":
      return (
        <span className="tags">
          {value.value.map((name) => (
            <span key={name} className="tag tag-accent">
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
    case "text":
    case "date":
      return <span>{value.value}</span>;
  }
}
