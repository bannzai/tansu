import { type ReactNode, useEffect, useState } from "react";
import { type Database, fetchDatabases } from "./api.ts";
import { APP_NAME } from "./appName.ts";
import { Link } from "./Link.tsx";
import { QaInput } from "./QaInput.tsx";
import { RowDetail } from "./RowDetail.tsx";
import { errorText } from "./relations.ts";
import { parseRoute, type Route, routePath } from "./route.ts";
import { TableView } from "./TableView.tsx";

/** 今の URL が指す画面。 */
function currentRoute(): Route {
  return parseRoute(window.location.pathname, window.location.search);
}

/**
 * Web 画面のルート。URL で画面を選ぶ。一覧と表ビューは左にデータベースの一覧を出し、行の詳細と QA の入力は
 * 画面いっぱいに出す (documents/design/mock-2026-10-11/ のモック)。
 */
export function App() {
  const [route, setRoute] = useState(currentRoute);
  const [databases, setDatabases] = useState<Database[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onPopState = () => setRoute(currentRoute());
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);
  useEffect(() => {
    fetchDatabases().then(setDatabases, (caught: unknown) => setError(errorText(caught)));
  }, []);

  if (error !== null) {
    return (
      <p role="alert" className="error page-error">
        {error}
      </p>
    );
  }
  if (databases === null) {
    return <p className="muted page-error">読み込み中</p>;
  }
  switch (route.page) {
    case "home":
      return (
        <Sidebar databases={databases} current={null}>
          <header className="page-header">
            <h1>データベース</h1>
            <span className="muted">{databases.length} 個</span>
          </header>
          {databases.length === 0 ? (
            <p className="muted">データベースはまだありません (tansu db create か tansu qa import で作れます)</p>
          ) : (
            <ul className="database-list">
              {databases.map((database) => (
                <li key={database.id}>
                  <Link to={routePath({ page: "table", database: database.name })}>{database.name}</Link>
                  <span className="muted small">
                    {database.properties.map((property) => property.name).join(" · ")}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Sidebar>
      );
    case "table":
      return (
        <Sidebar databases={databases} current={route.database}>
          <TableView key={route.database} databaseName={route.database} databases={databases} />
        </Sidebar>
      );
    case "row":
      return (
        <RowDetail
          key={`${route.database}/${route.rowId}`}
          databaseName={route.database}
          rowId={route.rowId}
          databases={databases}
        />
      );
    case "qa":
      return <QaInput repo={route.repo} feature={route.feature} />;
    case "not_found":
      return (
        <Sidebar databases={databases} current={null}>
          <p className="muted">この URL の画面はありません</p>
        </Sidebar>
      );
  }
}

/** 左のデータベースの一覧と QA の入力へのリンクを持つ枠。`current` は今開いているデータベースの名前。 */
function Sidebar({
  databases,
  current,
  children,
}: {
  databases: Database[];
  current: string | null;
  children: ReactNode;
}) {
  return (
    <div className="layout">
      <nav className="sidebar">
        <Link className="app-name" to="/">
          {APP_NAME}
        </Link>
        <div className="nav-heading">データベース</div>
        {databases.map((database) => (
          <Link
            key={database.id}
            className={database.name === current ? "nav-link nav-link-current" : "nav-link"}
            aria-current={database.name === current ? "page" : undefined}
            to={routePath({ page: "table", database: database.name })}
          >
            {database.name}
          </Link>
        ))}
        <div className="nav-heading">ビュー</div>
        <Link className="nav-link nav-link-view" to={routePath({ page: "qa", repo: null, feature: null })}>
          QA の入力
        </Link>
      </nav>
      <main className="content">{children}</main>
    </div>
  );
}
