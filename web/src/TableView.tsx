import { useEffect, useState } from "react";
import { type Database, type DatabaseRows, fetchRows, type Property } from "./api.ts";
import { Link } from "./Link.tsx";
import { errorText, fetchRelationTargets, relationTitles } from "./relations.ts";
import { routePath } from "./route.ts";
import { filterRows, type RowSort, rowTitle, sortRows, titleProperty } from "./rows.ts";
import { ValueView } from "./ValueView.tsx";

/** 選択肢で絞り込むプロパティの型 (選んだ値と一致・含む行に絞る)。それ以外の型は文字の部分一致で絞り込む。 */
const CHOICE_TYPES: ReadonlySet<string> = new Set(["select", "multi_select", "relation"]);

/** 表ビューが読み込んだ内容。 */
type Loaded = {
  /** 表示するデータベースの定義と行。 */
  table: DatabaseRows;
  /** relation の相手のデータベースの定義と行 (データベース ID → 内容)。 */
  targets: Map<string, DatabaseRows>;
};

/** データベース `databaseName` の表ビュー。プロパティの列、選択肢とテキストでの絞り込み、列の並べ替え。 */
export function TableView({ databaseName, databases }: { databaseName: string; databases: Database[] }) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [choiceFilters, setChoiceFilters] = useState<Record<string, string>>({});
  const [textProperty, setTextProperty] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [sort, setSort] = useState<RowSort | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const table = await fetchRows(databaseName);
      const targets = await fetchRelationTargets(table.database, databases);
      if (!cancelled) {
        setLoaded({ table, targets });
      }
    })().catch((caught: unknown) => {
      if (!cancelled) {
        setError(errorText(caught));
      }
    });
    return () => {
      cancelled = true;
    };
  }, [databaseName, databases]);

  if (error !== null) {
    return (
      <p role="alert" className="error">
        {error}
      </p>
    );
  }
  if (loaded === null) {
    return <p className="muted">読み込み中</p>;
  }
  const { database, rows } = loaded.table;
  const titles = relationTitles(loaded.targets);
  const title = titleProperty(database);
  const columns = database.properties.filter((property) => property !== title);
  const choiceProperties = database.properties.filter((property) => CHOICE_TYPES.has(property.kind.type));
  const textProperties = database.properties.filter(
    (property) => !CHOICE_TYPES.has(property.kind.type) && property.kind.type !== "checkbox",
  );
  const searchedProperty = textProperty ?? title?.name ?? textProperties[0]?.name;
  const filtered = filterRows(
    rows,
    [
      ...Object.entries(choiceFilters).map(([property, value]) => ({ property, value })),
      ...(searchedProperty === undefined ? [] : [{ property: searchedProperty, value: text }]),
    ],
    titles,
  );
  const shown = sort === null ? filtered : sortRows(filtered, sort, titles);
  const targetName = (property: Property) =>
    property.kind.type === "relation"
      ? loaded.targets.get(property.kind.target_database_id)?.database.name
      : undefined;
  const toggleSort = (propertyName: string) =>
    setSort((current) =>
      current?.property === propertyName && current.direction === "asc"
        ? { property: propertyName, direction: "desc" }
        : { property: propertyName, direction: "asc" },
    );
  const sortHeader = (propertyName: string) => (
    <th
      key={propertyName}
      aria-sort={
        sort?.property !== propertyName ? "none" : sort.direction === "asc" ? "ascending" : "descending"
      }
    >
      <button type="button" className="sort-button" onClick={() => toggleSort(propertyName)}>
        {propertyName}
        {sort?.property === propertyName ? (sort.direction === "asc" ? " ↑" : " ↓") : ""}
      </button>
    </th>
  );

  return (
    <div className="table-view">
      <header className="page-header">
        <h1>{database.name}</h1>
        <span className="muted" data-testid="row-count">
          {filtered.length === rows.length ? `${rows.length} 行` : `${filtered.length} / ${rows.length} 行`}
        </span>
      </header>
      <div className="filters">
        {choiceProperties.map((property) => (
          <div key={property.id} className={choiceFilters[property.name] ? "filter filter-active" : "filter"}>
            <span aria-hidden="true">{property.name}</span>
            <select
              aria-label={property.name}
              value={choiceFilters[property.name] ?? ""}
              onChange={(event) => setChoiceFilters({ ...choiceFilters, [property.name]: event.target.value })}
            >
              <option value="">すべて</option>
              {choiceOptions(property, loaded.targets).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </div>
        ))}
        {searchedProperty !== undefined && (
          <div className={text === "" ? "filter" : "filter filter-active"}>
            <select
              aria-label="テキストで絞り込むプロパティ"
              value={searchedProperty}
              onChange={(event) => setTextProperty(event.target.value)}
            >
              {textProperties.map((property) => (
                <option key={property.id} value={property.name}>
                  {property.name}
                </option>
              ))}
            </select>
            <input
              aria-label="テキスト"
              placeholder="含む文字"
              value={text}
              onChange={(event) => setText(event.target.value)}
            />
          </div>
        )}
        <button
          type="button"
          className="link-button"
          onClick={() => {
            setChoiceFilters({});
            setText("");
          }}
        >
          絞り込みを消す
        </button>
      </div>
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              {title === undefined ? <th>ID</th> : sortHeader(title.name)}
              {columns.map((property) => sortHeader(property.name))}
            </tr>
          </thead>
          <tbody>
            {shown.map((row) => (
              <tr key={row.id} data-testid="table-row" data-row-id={row.id}>
                <td>
                  <Link className="row-title" to={routePath({ page: "row", database: database.name, rowId: row.id })}>
                    {rowTitle(database, row)}
                  </Link>
                </td>
                {columns.map((property) => (
                  <td key={property.id}>
                    <ValueView
                      propertyName={property.name}
                      value={row.values[property.name]}
                      titles={titles}
                      targetDatabaseName={targetName(property)}
                    />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

/** 選択肢で絞り込むプロパティの (絞り込みの値, 見せる名前) の一覧。relation は相手の行 ID と行の名前。 */
function choiceOptions(property: Property, targets: ReadonlyMap<string, DatabaseRows>): [string, string][] {
  switch (property.kind.type) {
    case "select":
    case "multi_select":
      return property.kind.options.map((option) => [option.name, option.name]);
    case "relation": {
      const target = targets.get(property.kind.target_database_id);
      return target === undefined ? [] : target.rows.map((row) => [row.id, rowTitle(target.database, row)]);
    }
    default:
      return [];
  }
}
