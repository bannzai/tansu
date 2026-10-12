import { expect, test } from "vitest";
import type { QaSession, Row } from "./api.ts";
import { qaProgress, remainingMarkdown } from "./qa.ts";

const head = "3f2a9c1e5b7d4a6f8c0e2b4d6f8a1c3e5b7d9f0a";

/** 合成の item の行。 */
function item(id: string, name: string, expected: string): Row {
  return {
    id,
    body: "",
    values: { name: { type: "text", value: name }, expected: { type: "text", value: expected } },
    updated_at: "2026-10-12T00:00:00+09:00",
  };
}

/** 合成の run の行。 */
function run(result: string, commit: string, note?: string): Row {
  return {
    id: `run_${result}`,
    body: "",
    values: {
      result: { type: "select", value: result },
      commit: { type: "text", value: commit },
      ...(note === undefined ? {} : { note: { type: "text" as const, value: note } }),
    },
    updated_at: "2026-10-12T00:00:00+09:00",
  };
}

const session: QaSession = {
  repo: "sample-notes",
  feature: "note_list",
  head_commit: head,
  items: [
    { item: item("i1", "一覧の並び", "更新日時の新しい順に並ぶ"), latest_run: run("ok", head) },
    { item: item("i2", "検索で絞り込める", "含むノートだけが残る"), latest_run: run("ng", head, "1 文字目で\n絞り込まれない") },
    { item: item("i3", "空の案内", "案内文が出る"), latest_run: run("later", head) },
    // 別の commit で OK の項目は、今の HEAD では未入力
    { item: item("i4", "ピン留めの固定", "先頭に固定される"), latest_run: run("ok", "0".repeat(40)) },
    { item: item("i5", "スワイプで削除", "削除ボタンが出る"), latest_run: null },
  ],
};

test("今の HEAD で入力した項目だけを入力済みに数える", () => {
  expect(qaProgress(session)).toEqual({ entered: 3, total: 5, ok: 1, ng: 1, later: 1 });
});

test("全部コピーは未入力と NG の項目をメモ付きで markdown にする", () => {
  expect(remainingMarkdown(session)).toBe(
    [
      "## QA の残り: sample-notes / note_list (commit 3f2a9c1)",
      "### 未入力 (2 件)",
      "- [ ] 4. **ピン留めの固定**: 先頭に固定される\n- [ ] 5. **スワイプで削除**: 削除ボタンが出る",
      "### NG (1 件)",
      "- [ ] 2. **検索で絞り込める**: 含むノートだけが残る\n  - メモ: 1 文字目で\n    絞り込まれない",
    ].join("\n\n"),
  );
});

test("未入力も NG も無ければその旨を書く", () => {
  const done: QaSession = { ...session, items: session.items.slice(0, 1) };
  expect(remainingMarkdown(done)).toBe(
    "## QA の残り: sample-notes / note_list (commit 3f2a9c1)\n\n未入力と NG の項目はありません",
  );
});
