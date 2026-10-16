import { type KeyboardEvent as ReactKeyboardEvent, type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import {
  type Database,
  type DatabaseRows,
  fetchRows,
  type Property,
  type PropertyKind,
  type Row,
  type RowValueInput,
  upsertRow,
} from "./api.ts";
import { RowDetail } from "./RowDetail.tsx";
import { errorText, fetchRelationTargets, relationTitles } from "./relations.ts";
import { navigate, routePath } from "./route.ts";
import {
  changedValues,
  filterRows,
  inputValue,
  newRowId,
  type RowSort,
  type RowTitles,
  rowTitle,
  sortRows,
  titleProperty,
} from "./rows.ts";
import { ValueView } from "./ValueView.tsx";

/** 選択肢で絞り込むプロパティの型 (選んだ値と一致・含む行に絞る)。それ以外の型は文字の部分一致で絞り込む。 */
const CHOICE_TYPES: ReadonlySet<string> = new Set(["select", "multi_select", "relation"]);

/** 列見出しに出すプロパティの型のアイコン (Notion のテーブルビューの列見出しに合わせ、文字で描く)。 */
const PROPERTY_ICONS: Record<PropertyKind["type"], string> = {
  text: "Aa",
  number: "#",
  select: "◉",
  multi_select: "☰",
  checkbox: "☑",
  url: "↗",
  date: "▦",
  relation: "⇄",
};

/** 表ビューが読み込んだ内容。 */
type Loaded = {
  /** 表示するデータベースの定義と行。 */
  table: DatabaseRows;
  /** relation の相手のデータベースの定義と行 (データベース ID → 内容)。 */
  targets: Map<string, DatabaseRows>;
};

/** その場で編集しているセル。 */
type EditingCell = {
  /** 行 ID。 */
  rowId: string;
  /** プロパティの名前。 */
  property: string;
};

/**
 * データベース `databaseName` の表ビュー (Notion のテーブルビューに寄せた格子の表)。列見出しに型のアイコン、セルは型に
 * 合わせて描き、クリックでその場で編集する。絞り込みと並べ替えは表の上のツールバー、最後の行の「+ 新規」で空の行を足す。
 * `peek` は右のパネル (サイドピーク) に開いている行の ID (URL の `?peek=`)。
 */
export function TableView({
  databaseName,
  databases,
  peek,
}: {
  databaseName: string;
  databases: Database[];
  peek: string | null;
}) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [writeError, setWriteError] = useState<string | null>(null);
  const [choiceFilters, setChoiceFilters] = useState<Record<string, string>>({});
  const [textProperty, setTextProperty] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [sort, setSort] = useState<RowSort | null>(null);
  const [toolbarPanel, setToolbarPanel] = useState<"filter" | "sort" | null>(null);
  const [editing, setEditing] = useState<EditingCell | null>(null);
  const [adding, setAdding] = useState(false);
  // 表のセルで最後に書いた行。パネルに開いている行なら、書いた値をパネルに映す
  const [lastWrittenRow, setLastWrittenRow] = useState<Row | undefined>(undefined);

  const load = useCallback(async () => {
    const table = await fetchRows(databaseName);
    setLoaded({ table, targets: await fetchRelationTargets(table.database, databases) });
  }, [databaseName, databases]);

  useEffect(() => {
    load().catch((caught: unknown) => setError(errorText(caught)));
  }, [load]);

  useEffect(() => {
    if (peek === null) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      // 編集欄の Esc はその編集の取消に使うため、パネルを閉じない
      if (event.key !== "Escape" || isTextEntry(event.target)) {
        return;
      }
      navigate(routePath({ page: "table", database: databaseName, peek: null }));
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [peek, databaseName]);

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
  // 編集中の行は絞り込みに合わなくなっても残す (「+ 新規」で足した空の行に名前を入れられるように)
  const visible =
    editing === null || filtered.some((row) => row.id === editing.rowId)
      ? filtered
      : [...filtered, ...rows.filter((row) => row.id === editing.rowId)];
  const shown = sort === null ? visible : sortRows(visible, sort, titles);
  const activeFilterCount = Object.values(choiceFilters).filter((value) => value !== "").length + (text === "" ? 0 : 1);
  const targetName = (property: Property) =>
    property.kind.type === "relation" ? loaded.targets.get(property.kind.target_database_id)?.database.name : undefined;
  const replaceRow = (written: Row) =>
    setLoaded((current) =>
      current === null
        ? current
        : {
            ...current,
            table: {
              ...current.table,
              rows: current.table.rows.some((row) => row.id === written.id)
                ? current.table.rows.map((row) => (row.id === written.id ? written : row))
                : [...current.table.rows, written],
            },
          },
    );

  const writeValue = async (row: Row, property: Property, value: RowValueInput) => {
    setEditing(null);
    // 表を読み込んだ後に CLI や別のタブが書いた値を消さないよう、変えたプロパティだけを送る
    const values = changedValues(row, { [property.name]: value });
    if (Object.keys(values).length === 0) {
      return;
    }
    setWriteError(null);
    try {
      const written = await upsertRow(database.name, { id: row.id, values });
      replaceRow(written);
      setLastWrittenRow(written);
    } catch (caught) {
      setWriteError(errorText(caught));
    }
  };

  const addRow = async () => {
    setWriteError(null);
    setAdding(true);
    try {
      const written = await upsertRow(database.name, { id: newRowId(), values: {} });
      replaceRow(written);
      if (title !== undefined) {
        setEditing({ rowId: written.id, property: title.name });
      }
    } catch (caught) {
      setWriteError(errorText(caught));
    } finally {
      setAdding(false);
    }
  };

  const ariaSort = (propertyName: string) =>
    sort?.property !== propertyName ? "none" : sort.direction === "asc" ? "ascending" : "descending";
  const header = (property: Property) => (
    <th key={property.id} aria-sort={ariaSort(property.name)} data-property-type={property.kind.type}>
      <span className="column-header">
        <span className="property-icon" aria-hidden="true">
          {PROPERTY_ICONS[property.kind.type]}
        </span>
        {property.name}
        {sort?.property === property.name && <span aria-hidden="true">{sort.direction === "asc" ? " ↑" : " ↓"}</span>}
      </span>
    </th>
  );
  const cell = (row: Row, property: Property, extra?: ReactNode) => (
    <Cell
      key={property.id}
      row={row}
      rowLabel={rowTitle(database, row)}
      property={property}
      titles={titles}
      targetDatabaseName={targetName(property)}
      editing={editing?.rowId === row.id && editing.property === property.name}
      onEdit={() => setEditing({ rowId: row.id, property: property.name })}
      onCancel={() => setEditing(null)}
      onWrite={(value) => writeValue(row, property, value)}
      extra={extra}
    />
  );

  return (
    <div className={peek === null ? "table-view" : "table-view table-view-peeking"}>
      <header className="page-header">
        <h1>{database.name}</h1>
        <span className="muted" data-testid="row-count">
          {filtered.length === rows.length ? `${rows.length} 行` : `${filtered.length} / ${rows.length} 行`}
        </span>
      </header>
      <div className="toolbar">
        <button
          type="button"
          className={activeFilterCount > 0 ? "toolbar-button toolbar-button-active" : "toolbar-button"}
          aria-expanded={toolbarPanel === "filter"}
          onClick={() => setToolbarPanel(toolbarPanel === "filter" ? null : "filter")}
        >
          フィルター{activeFilterCount > 0 && ` ${activeFilterCount}`}
        </button>
        <button
          type="button"
          className={sort === null ? "toolbar-button" : "toolbar-button toolbar-button-active"}
          aria-expanded={toolbarPanel === "sort"}
          onClick={() => setToolbarPanel(toolbarPanel === "sort" ? null : "sort")}
        >
          並べ替え
        </button>
        {writeError !== null && (
          <span role="alert" className="error">
            {writeError}
          </span>
        )}
      </div>
      {toolbarPanel === "filter" && (
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
      )}
      {toolbarPanel === "sort" && (
        <div className="filters">
          <div className={sort === null ? "filter" : "filter filter-active"}>
            <select
              aria-label="並べ替えるプロパティ"
              value={sort?.property ?? ""}
              onChange={(event) =>
                setSort(
                  event.target.value === ""
                    ? null
                    : { property: event.target.value, direction: sort?.direction ?? "asc" },
                )
              }
            >
              <option value="">(並べ替えない)</option>
              {database.properties.map((property) => (
                <option key={property.id} value={property.name}>
                  {property.name}
                </option>
              ))}
            </select>
            <select
              aria-label="並べ替えの向き"
              value={sort?.direction ?? "asc"}
              disabled={sort === null}
              onChange={(event) =>
                setSort(sort === null ? null : { ...sort, direction: event.target.value === "desc" ? "desc" : "asc" })
              }
            >
              <option value="asc">昇順</option>
              <option value="desc">降順</option>
            </select>
          </div>
        </div>
      )}
      <div className="table-scroll">
        <table className="grid">
          <thead>
            <tr>
              {title === undefined ? <th>ID</th> : header(title)}
              {columns.map(header)}
            </tr>
          </thead>
          <tbody>
            {shown.map((row) => {
              const openButton = (
                <OpenButton
                  label={rowTitle(database, row)}
                  onOpen={() => navigate(routePath({ page: "table", database: database.name, peek: row.id }))}
                />
              );
              return (
                <tr
                  key={row.id}
                  data-testid="table-row"
                  data-row-id={row.id}
                  className={row.id === peek ? "row-peeked" : undefined}
                >
                  {title === undefined ? (
                    <td className="cell title-cell">
                      <span className="cell-value row-title">{row.id}</span>
                      {openButton}
                    </td>
                  ) : (
                    cell(row, title, openButton)
                  )}
                  {columns.map((property) => cell(row, property))}
                </tr>
              );
            })}
            <tr>
              <td colSpan={columns.length + 1} className="new-row-cell">
                <button type="button" className="new-row-button" disabled={adding} onClick={addRow}>
                  + 新規
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      {peek !== null && (
        <section className="peek" aria-label="行のページ" data-testid="peek">
          <RowDetail
            key={peek}
            savedRow={lastWrittenRow}
            databaseName={database.name}
            rowId={peek}
            databases={databases}
            onClose={() => navigate(routePath({ page: "table", database: database.name, peek: null }))}
            onSaved={() => {
              load().catch((caught: unknown) => setWriteError(errorText(caught)));
            }}
          />
        </section>
      )}
    </div>
  );
}

/** キー入力が文字の入力に使われる要素か (Esc をその要素の編集の取消に残すため)。 */
function isTextEntry(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.matches("textarea, select, input:not([type=checkbox])") || target.isContentEditable)
  );
}

/** 行の名前の列の行頭に出す「開く」。押すと右のパネルにその行のページを開く。`label` は行の名前。 */
function OpenButton({ label, onOpen }: { label: string; onOpen: () => void }) {
  return (
    <button type="button" className="open-button" aria-label={`${label} を開く`} onClick={onOpen}>
      開く
    </button>
  );
}

/**
 * 表の 1 つのセル。checkbox はその場で切り替え、relation は読み取りだけ (詳細ページと同じく編集しない)、それ以外は
 * クリックで `editing` にして編集欄を出す。`onWrite` は編集した値を保存し、`onCancel` は保存せずに編集をやめる。
 * `extra` は値の後に置く要素 (行の名前の列の「開く」)。`rowLabel` は行の名前で、操作の名前に使う。
 */
function Cell({
  row,
  rowLabel,
  property,
  titles,
  targetDatabaseName,
  editing,
  onEdit,
  onCancel,
  onWrite,
  extra,
}: {
  row: Row;
  rowLabel: string;
  property: Property;
  titles: RowTitles;
  targetDatabaseName: string | undefined;
  editing: boolean;
  onEdit: () => void;
  onCancel: () => void;
  onWrite: (value: RowValueInput) => void;
  extra?: ReactNode;
}) {
  const value = row.values[property.name];
  const label = `${rowLabel} の ${property.name}`;
  const className = extra === undefined ? "cell" : "cell title-cell";
  if (property.kind.type === "checkbox") {
    return (
      <td className={className}>
        <input
          type="checkbox"
          aria-label={label}
          checked={value?.type === "checkbox" && value.value}
          onChange={(event) => onWrite(event.target.checked)}
        />
      </td>
    );
  }
  if (editing && property.kind.type !== "relation") {
    return (
      <td className={`${className} cell-editing`}>
        <CellEditor
          property={property}
          initial={inputValue(value)}
          label={label}
          onWrite={onWrite}
          onCancel={onCancel}
        />
      </td>
    );
  }
  return (
    <td className={className}>
      {property.kind.type !== "relation" && (
        <button type="button" className="cell-edit" aria-label={`${label} を編集`} onClick={onEdit} />
      )}
      <span className={extra === undefined ? "cell-value" : "cell-value row-title"}>
        {extra !== undefined && (value === undefined || (value.type === "text" && value.value === "")) ? (
          <span className="muted">無題</span>
        ) : (
          <ValueView
            propertyName={property.name}
            value={value}
            titles={titles}
            targetDatabaseName={targetDatabaseName}
          />
        )}
      </span>
      {extra}
    </td>
  );
}

/** 編集欄を描いた時にフォーカスを移す ref。 */
function focusOnMount(element: HTMLElement | null) {
  element?.focus();
}

/**
 * セルの編集欄。text / number / url / date は入力欄で Enter (text は Shift + Enter で改行) かフォーカスを外すと保存、
 * select は選ぶと保存、multi_select はチェックを変えて「完了」かフォーカスを外すと保存する。Esc は保存せずにやめる。
 * `initial` は編集前の値 (upsert に渡す形)。`label` は編集欄の名前。
 */
function CellEditor({
  property,
  initial,
  label,
  onWrite,
  onCancel,
}: {
  property: Property;
  initial: RowValueInput;
  label: string;
  onWrite: (value: RowValueInput) => void;
  onCancel: () => void;
}) {
  const [draftText, setDraftText] = useState(
    typeof initial === "string" || typeof initial === "number" ? String(initial) : "",
  );
  const [draftChoices, setDraftChoices] = useState(Array.isArray(initial) ? initial : []);
  // 保存か取消をした後に、編集欄が消える時の blur でもう一度保存しないようにする
  const finished = useRef(false);
  const finish = (value: RowValueInput | undefined) => {
    if (finished.current) {
      return;
    }
    finished.current = true;
    if (value === undefined) {
      onCancel();
    } else {
      onWrite(value);
    }
  };
  // 編集欄の Esc はこの編集の取消だけに使い、パネルを閉じる window の Esc まで届けない
  // (multi_select の「完了」やチェックボックスは文字の入力欄ではないため、window 側では見分けられない)
  const cancel = (event: ReactKeyboardEvent) => {
    event.stopPropagation();
    finish(undefined);
  };
  const textValue = () =>
    property.kind.type === "number" ? (draftText.trim() === "" ? null : Number(draftText)) : draftText;

  switch (property.kind.type) {
    case "select":
      return (
        <select
          ref={focusOnMount}
          aria-label={label}
          className="cell-input"
          value={typeof initial === "string" ? initial : ""}
          onChange={(event) => finish(event.target.value === "" ? null : event.target.value)}
          onBlur={() => finish(undefined)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              cancel(event);
            }
          }}
        >
          <option value="">(なし)</option>
          {property.kind.options.map((option) => (
            <option key={option.id} value={option.name}>
              {option.name}
            </option>
          ))}
        </select>
      );
    case "multi_select":
      return (
        <fieldset
          className="cell-popover"
          aria-label={label}
          onBlur={(event) => {
            if (!(event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget))) {
              finish(draftChoices);
            }
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              cancel(event);
            }
          }}
        >
          {property.kind.options.length === 0 && <span className="muted small">選択肢はまだありません</span>}
          {property.kind.options.map((option) => (
            <label key={option.id} className="check">
              <input
                type="checkbox"
                checked={draftChoices.includes(option.name)}
                onChange={(event) =>
                  setDraftChoices(
                    event.target.checked
                      ? [...draftChoices, option.name]
                      : draftChoices.filter((name) => name !== option.name),
                  )
                }
              />
              {option.name}
            </label>
          ))}
          <button type="button" ref={focusOnMount} className="link-button" onClick={() => finish(draftChoices)}>
            完了
          </button>
        </fieldset>
      );
    case "text":
      return (
        <textarea
          ref={focusOnMount}
          aria-label={label}
          className="cell-input"
          rows={3}
          value={draftText}
          onChange={(event) => setDraftText(event.target.value)}
          onBlur={() => finish(draftText)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              finish(draftText);
            } else if (event.key === "Escape") {
              cancel(event);
            }
          }}
        />
      );
    default:
      return (
        <input
          ref={focusOnMount}
          aria-label={label}
          className="cell-input"
          type={property.kind.type === "number" ? "number" : "text"}
          placeholder={property.kind.type === "date" ? "2026-10-12" : property.kind.type === "url" ? "https://" : ""}
          value={draftText}
          onChange={(event) => setDraftText(event.target.value)}
          onBlur={() => finish(textValue())}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
              finish(textValue());
            } else if (event.key === "Escape") {
              cancel(event);
            }
          }}
        />
      );
  }
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
