import { expect, test } from "vitest";
import { tagColor } from "./tags.ts";

test("同じ選択肢の名前はいつも同じ色で、色の番号は 0〜8", () => {
  for (const name of ["auto", "manual", "todo", "課金", "", "とても長い選択肢の名前".repeat(20)]) {
    expect(tagColor(name)).toBe(tagColor(name));
    expect(tagColor(name)).toBeGreaterThanOrEqual(0);
    expect(tagColor(name)).toBeLessThan(9);
  }
});

test("違う名前の選択肢は 1〜2 色に偏らない", () => {
  const colors = new Set(["auto", "manual", "todo", "ok", "ng", "later", "skip", "record", "import"].map(tagColor));
  expect(colors.size).toBeGreaterThan(2);
});
