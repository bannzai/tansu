import { expect, test } from "vitest";
import type { Database, Row } from "./api.ts";
import { filterRows, rowTitle, sortRows, valueText } from "./rows.ts";

const item: Database = {
  id: "db_item",
  name: "item",
  properties: [
    { id: "p_feature", name: "feature", kind: { type: "relation", target_database_id: "db_feature" } },
    { id: "p_name", name: "name", kind: { type: "text" } },
    {
      id: "p_automation",
      name: "automation",
      kind: {
        type: "select",
        options: [
          { id: "o_auto", name: "auto" },
          { id: "o_manual", name: "manual" },
        ],
      },
    },
    { id: "p_labels", name: "labels", kind: { type: "multi_select", options: [{ id: "o_billing", name: "課金" }] } },
    { id: "p_points", name: "points", kind: { type: "number" } },
  ],
};

/** 合成の item の行。 */
function row(id: string, values: Row["values"]): Row {
  return { id, body: "", values, updated_at: "2026-10-12T00:00:00+09:00" };
}

const rows = [
  row("r1", {
    name: { type: "text", value: "Premium の購入" },
    automation: { type: "select", value: "manual" },
    labels: { type: "multi_select", value: ["課金"] },
    feature: { type: "relation", value: ["f_billing"] },
    points: { type: "number", value: 10 },
  }),
  row("r2", {
    name: { type: "text", value: "一覧の並び" },
    automation: { type: "select", value: "auto" },
    feature: { type: "relation", value: ["f_list"] },
    points: { type: "number", value: 2 },
  }),
  row("r3", { name: { type: "text", value: "購入の復元" }, automation: { type: "select", value: "manual" } }),
];

const titles = new Map([
  ["f_billing", "billing"],
  ["f_list", "note_list"],
]);

test("select は一致、multi_select と relation は含む行、テキストは部分一致で絞り込む", () => {
  const ids = (filters: Parameters<typeof filterRows>[1]) => filterRows(rows, filters, titles).map((r) => r.id);
  expect(ids([{ property: "automation", value: "manual" }])).toEqual(["r1", "r3"]);
  expect(ids([{ property: "labels", value: "課金" }])).toEqual(["r1"]);
  expect(ids([{ property: "feature", value: "f_list" }])).toEqual(["r2"]);
  expect(ids([{ property: "name", value: "購入" }])).toEqual(["r1", "r3"]);
  expect(ids([{ property: "name", value: "premium" }])).toEqual(["r1"]);
  // 空の値は絞り込まない
  expect(ids([{ property: "name", value: "" }])).toEqual(["r1", "r2", "r3"]);
  expect(
    ids([
      { property: "automation", value: "manual" },
      { property: "name", value: "復元" },
    ]),
  ).toEqual(["r3"]);
});

test("number は数で並べ、値の無い行は向きによらず最後に置く", () => {
  const ids = (direction: "asc" | "desc") =>
    sortRows(rows, { property: "points", direction }, titles).map((r) => r.id);
  expect(ids("asc")).toEqual(["r2", "r1", "r3"]);
  expect(ids("desc")).toEqual(["r1", "r2", "r3"]);
});

test("relation は相手の行の名前で並べる", () => {
  expect(sortRows(rows, { property: "feature", direction: "asc" }, titles).map((r) => r.id)).toEqual([
    "r1",
    "r2",
    "r3",
  ]);
  expect(valueText(rows[1]?.values.feature, titles)).toBe("note_list");
});

test("行の名前は name の値、無ければ行 ID", () => {
  expect(rowTitle(item, rows[0] as Row)).toBe("Premium の購入");
  expect(rowTitle(item, row("r9", {}))).toBe("r9");
});
