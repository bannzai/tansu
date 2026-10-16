/** 画面の URL が指す画面。 */
export type Route =
  | { page: "home" }
  /** `peek` は表の右のパネル (サイドピーク) に開いている行の ID。開いていなければ null。 */
  | { page: "table"; database: string; peek: string | null }
  | { page: "row"; database: string; rowId: string }
  | { page: "qa"; repo: string | null; feature: string | null }
  | { page: "not_found" };

/** URL のパス `pathname` と問い合わせ `search` が指す画面。 */
export function parseRoute(pathname: string, search: string): Route {
  const segments = pathname
    .split("/")
    .filter((segment) => segment !== "")
    .map(decodeURIComponent);
  const [first, database, rowId, ...rest] = segments;
  if (first === undefined) {
    return { page: "home" };
  }
  if (first === "qa" && database === undefined) {
    const params = new URLSearchParams(search);
    return { page: "qa", repo: params.get("repo"), feature: params.get("feature") };
  }
  if (first === "db" && database !== undefined && rest.length === 0) {
    return rowId === undefined
      ? { page: "table", database, peek: new URLSearchParams(search).get("peek") || null }
      : { page: "row", database, rowId };
  }
  return { page: "not_found" };
}

/** 画面 `route` の URL (パスと問い合わせ)。 */
export function routePath(route: Route): string {
  switch (route.page) {
    case "home":
    case "not_found":
      return "/";
    case "table": {
      const path = `/db/${encodeURIComponent(route.database)}`;
      return route.peek === null ? path : `${path}?${new URLSearchParams({ peek: route.peek })}`;
    }
    case "row":
      return `/db/${encodeURIComponent(route.database)}/${encodeURIComponent(route.rowId)}`;
    case "qa": {
      const params = new URLSearchParams();
      if (route.repo !== null) {
        params.set("repo", route.repo);
      }
      if (route.feature !== null) {
        params.set("feature", route.feature);
      }
      const query = params.toString();
      return query === "" ? "/qa" : `/qa?${query}`;
    }
  }
}

/** 履歴に `path` を積んで画面を移る。`popstate` を送り、App が URL を読み直す。 */
export function navigate(path: string): void {
  window.history.pushState(null, "", path);
  window.dispatchEvent(new PopStateEvent("popstate"));
}
