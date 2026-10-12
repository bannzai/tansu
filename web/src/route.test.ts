import { expect, test } from "vitest";
import { parseRoute, routePath } from "./route.ts";

test("URL を画面に読み、画面から同じ URL に戻せる", () => {
  for (const route of [
    { page: "home" },
    { page: "table", database: "item" },
    { page: "row", database: "run", rowId: "V1StGXR8_Z5jdHi6Bmy" },
    { page: "qa", repo: "acme/notes", feature: "note_list" },
    { page: "qa", repo: null, feature: null },
  ] as const) {
    const [pathname, search = ""] = routePath(route).split("?");
    expect(parseRoute(pathname ?? "", search)).toEqual(route);
  }
});

test("日本語や記号を含むデータベース名も URL を通して戻る", () => {
  expect(parseRoute(routePath({ page: "table", database: "メモ/下書き" }), "")).toEqual({
    page: "table",
    database: "メモ/下書き",
  });
});

test("知らないパスは not_found", () => {
  expect(parseRoute("/unknown", "")).toEqual({ page: "not_found" });
  expect(parseRoute("/db/item/row/extra", "")).toEqual({ page: "not_found" });
  expect(parseRoute("/db", "")).toEqual({ page: "not_found" });
});
