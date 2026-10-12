import { useCallback, useEffect, useState } from "react";
import Markdown from "react-markdown";
import {
  addProperties,
  type Database,
  type DatabaseRows,
  fetchRows,
  type NamedValue,
  type Property,
  type Row,
  type RowValueInput,
  upsertRow,
} from "./api.ts";
import { Link } from "./Link.tsx";
import { errorText, fetchRelationTargets, relationTitles } from "./relations.ts";
import { routePath } from "./route.ts";
import { type RowTitles, rowTitle } from "./rows.ts";
import { ValueView } from "./ValueView.tsx";

/** 詳細ページが読み込んだ内容。 */
type Loaded = {
  /** 行のデータベースの定義。 */
  database: Database;
  /** 表示している行。 */
  row: Row;
  /** relation の相手のデータベースの定義と行 (データベース ID → 内容)。 */
  targets: Map<string, DatabaseRows>;
  /** この行を relation で参照している行。参照しているプロパティごと。 */
  backlinks: { database: Database; property: Property; rows: Row[] }[];
};

/** 行の値を、編集欄が持つ形 (upsert に渡す値) にする。relation は詳細ページでは編集しないので扱わない。 */
function inputValue(value: NamedValue | undefined): RowValueInput {
  return value === undefined ? null : value.value;
}

/** データベース `databaseName` の行 `rowId` の詳細ページ。プロパティと markdown の本文を編集し、参照している行を並べる。 */
export function RowDetail({
  databaseName,
  rowId,
  databases,
}: {
  databaseName: string;
  rowId: string;
  databases: Database[];
}) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [draft, setDraft] = useState<Record<string, RowValueInput>>({});
  const [body, setBody] = useState("");
  const [editingBody, setEditingBody] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    const { database, rows } = await fetchRows(databaseName);
    const row = rows.find((candidate) => candidate.id === rowId);
    if (row === undefined) {
      throw new Error(`データベース ${databaseName} に行 ${rowId} はありません`);
    }
    setLoaded({
      database,
      row,
      targets: await fetchRelationTargets(database, databases),
      backlinks: await Promise.all(
        databases
          .flatMap((other) =>
            other.properties
              .filter(
                (property) => property.kind.type === "relation" && property.kind.target_database_id === database.id,
              )
              .map((property) => ({ database: other, property })),
          )
          .map(async ({ database: other, property }) => ({
            database: other,
            property,
            rows: (await fetchRows(other.name)).rows.filter((candidate) => {
              const value = candidate.values[property.name];
              return value?.type === "relation" && value.value.includes(row.id);
            }),
          })),
      ),
    });
    setDraft(
      Object.fromEntries(
        database.properties
          .filter((property) => property.kind.type !== "relation")
          .map((property) => [property.name, inputValue(row.values[property.name])]),
      ),
    );
    setBody(row.body);
  }, [databaseName, rowId, databases]);

  useEffect(() => {
    load().catch((caught: unknown) => setError(errorText(caught)));
  }, [load]);

  if (loaded === null) {
    return error === null ? (
      <p className="muted">読み込み中</p>
    ) : (
      <p role="alert" className="error">
        {error}
      </p>
    );
  }
  const { database, row } = loaded;
  const titles = relationTitles(loaded.targets);

  const save = async () => {
    setError(null);
    setStatus(null);
    try {
      await upsertRow(database.name, {
        id: row.id,
        body,
        values: Object.fromEntries(Object.entries(draft).map(([name, value]) => [name, value === "" ? null : value])),
      });
      await load();
      setEditingBody(false);
      setStatus("保存しました");
    } catch (caught) {
      setError(errorText(caught));
    }
  };

  const addOption = async (property: Property, optionName: string) => {
    setError(null);
    try {
      const updated = await addProperties(database.name, [`${property.name}:${property.kind.type}:${optionName}`]);
      setLoaded({ ...loaded, database: updated });
      const current = draft[property.name];
      setDraft({
        ...draft,
        [property.name]:
          property.kind.type === "multi_select" ? [...(Array.isArray(current) ? current : []), optionName] : optionName,
      });
    } catch (caught) {
      setError(errorText(caught));
    }
  };

  return (
    <div className="detail">
      <header className="bar">
        <Link to={routePath({ page: "table", database: database.name })}>← {database.name}</Link>
        <div className="spacer" />
        <span className="muted small">
          ID {row.id} · 更新 {row.updated_at}
        </span>
      </header>
      <main className="detail-main">
        <section className="detail-body">
          <h1>{rowTitle(database, row)}</h1>
          <div className="properties">
            {database.properties.map((property) => (
              <div key={property.id} className="property-row">
                <span className="property-name">{property.name}</span>
                <div className="property-value">
                  {property.kind.type === "relation" ? (
                    <ValueView
                      propertyName={property.name}
                      value={row.values[property.name]}
                      titles={titles}
                      targetDatabaseName={loaded.targets.get(property.kind.target_database_id)?.database.name}
                    />
                  ) : (
                    <PropertyEditor
                      property={property}
                      value={draft[property.name] ?? null}
                      onChange={(value) => setDraft({ ...draft, [property.name]: value })}
                      onAddOption={(optionName) => addOption(property, optionName)}
                    />
                  )}
                </div>
              </div>
            ))}
          </div>
          <div className="body-section">
            <div className="section-header">
              <span className="section-title">本文</span>
              <button type="button" className="link-button" onClick={() => setEditingBody(!editingBody)}>
                {editingBody ? "プレビュー" : "本文を編集"}
              </button>
            </div>
            {editingBody ? (
              <textarea
                aria-label="本文"
                className="body-editor"
                rows={10}
                value={body}
                onChange={(event) => setBody(event.target.value)}
              />
            ) : (
              <div className="markdown" data-testid="row-body">
                {body === "" ? <p className="muted">本文はありません</p> : <MarkdownBody markdown={body} />}
              </div>
            )}
          </div>
          <div className="actions">
            <button type="button" className="primary" onClick={save}>
              保存
            </button>
            {status !== null && (
              <span role="status" className="muted">
                {status}
              </span>
            )}
            {error !== null && (
              <span role="alert" className="error">
                {error}
              </span>
            )}
          </div>
        </section>
        <aside className="backlinks">
          {loaded.backlinks.map(({ database: other, property, rows }) => (
            <section key={`${other.id}:${property.id}`} className="backlink-group">
              <div className="section-title">
                {other.name} ({rows.length} 件)
              </div>
              {[...rows].reverse().map((referring) => (
                <BacklinkCard key={referring.id} database={other} property={property} row={referring} titles={titles} />
              ))}
            </section>
          ))}
        </aside>
      </main>
    </div>
  );
}

/**
 * markdown の本文。画像は読み込まずにリンクにする (外部への送信を DB の接続だけにする制約。documents/PROJECT.md「制約」)。
 * 生の HTML は react-markdown が描かない。
 */
function MarkdownBody({ markdown }: { markdown: string }) {
  return (
    <Markdown
      components={{
        img: ({ src, alt }) =>
          typeof src === "string" ? (
            <a href={src} target="_blank" rel="noreferrer">
              {alt || src}
            </a>
          ) : null,
        a: ({ href, children }) => (
          <a href={href} target="_blank" rel="noreferrer">
            {children}
          </a>
        ),
      }}
    >
      {markdown}
    </Markdown>
  );
}

/** この行を参照している行 `row` の札。参照しているプロパティ `property` 以外の値を並べる。 */
function BacklinkCard({
  database,
  property,
  row,
  titles,
}: {
  database: Database;
  property: Property;
  row: Row;
  titles: RowTitles;
}) {
  return (
    <div className="card" data-testid="backlink">
      <Link to={routePath({ page: "row", database: database.name, rowId: row.id })}>{rowTitle(database, row)}</Link>
      <div className="card-values">
        {database.properties
          .filter((other) => other.id !== property.id && row.values[other.name] !== undefined)
          .map((other) => (
            <span key={other.id} className="card-value">
              <span className="muted small">{other.name}</span>{" "}
              <ValueView
                propertyName={other.name}
                value={row.values[other.name]}
                titles={titles}
                targetDatabaseName={undefined}
              />
            </span>
          ))}
      </div>
    </div>
  );
}

/** relation 以外のプロパティの編集欄。select / multi_select は選択肢を足せる。 */
function PropertyEditor({
  property,
  value,
  onChange,
  onAddOption,
}: {
  property: Property;
  value: RowValueInput;
  onChange: (value: RowValueInput) => void;
  onAddOption: (optionName: string) => void;
}) {
  const [newOption, setNewOption] = useState("");
  const addOptionInput = (
    <span className="add-option">
      <input
        aria-label={`${property.name} に足す選択肢`}
        placeholder="新しい選択肢"
        value={newOption}
        onChange={(event) => setNewOption(event.target.value)}
      />
      <button
        type="button"
        disabled={newOption.trim() === "" || newOption.includes(",")}
        onClick={() => {
          onAddOption(newOption.trim());
          setNewOption("");
        }}
      >
        足す
      </button>
    </span>
  );
  switch (property.kind.type) {
    case "select":
      return (
        <span className="editor-row">
          <select
            aria-label={property.name}
            value={typeof value === "string" ? value : ""}
            onChange={(event) => onChange(event.target.value === "" ? null : event.target.value)}
          >
            <option value="">(なし)</option>
            {property.kind.options.map((option) => (
              <option key={option.id} value={option.name}>
                {option.name}
              </option>
            ))}
          </select>
          {addOptionInput}
        </span>
      );
    case "multi_select": {
      const selected = Array.isArray(value) ? value : [];
      return (
        <fieldset className="editor-row" aria-label={property.name}>
          {property.kind.options.map((option) => (
            <label key={option.id} className="check">
              <input
                type="checkbox"
                checked={selected.includes(option.name)}
                onChange={(event) =>
                  onChange(
                    event.target.checked ? [...selected, option.name] : selected.filter((name) => name !== option.name),
                  )
                }
              />
              {option.name}
            </label>
          ))}
          {addOptionInput}
        </fieldset>
      );
    }
    case "checkbox":
      return (
        <input
          type="checkbox"
          aria-label={property.name}
          checked={value === true}
          onChange={(event) => onChange(event.target.checked)}
        />
      );
    case "number":
      return (
        <input
          type="number"
          aria-label={property.name}
          value={typeof value === "number" ? value : ""}
          onChange={(event) => onChange(event.target.value === "" ? null : Number(event.target.value))}
        />
      );
    case "text":
      return (
        <textarea
          aria-label={property.name}
          rows={1}
          className="text-editor"
          value={typeof value === "string" ? value : ""}
          onChange={(event) => onChange(event.target.value)}
        />
      );
    case "url":
    case "date":
      return (
        <input
          aria-label={property.name}
          placeholder={property.kind.type === "date" ? "2026-10-12" : "https://"}
          value={typeof value === "string" ? value : ""}
          onChange={(event) => onChange(event.target.value)}
        />
      );
    case "relation":
      return null;
  }
}
