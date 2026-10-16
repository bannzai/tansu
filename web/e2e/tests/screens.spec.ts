import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, type Page, test } from "@playwright/test";
import { tansuHome } from "../paths.ts";

// fixtures/qa/sample-notes を取り込んだ 1 つの DB を、上のテストから順に書き換えながら確かめる
// (詳細ページで足した label を、表ビューの絞り込みが使う)。
test.describe.configure({ mode: "serial" });

/** 利用記録 (usage.jsonl) の `web_open` の行の数。 */
function webOpenCount(): number {
  return readFileSync(path.join(tansuHome, "usage.jsonl"), "utf8")
    .split("\n")
    .filter((line) => line !== "" && (JSON.parse(line) as { event: string }).event === "web_open").length;
}

/** 表ビューの行の、名前の列の文字列 (上から順)。 */
async function rowTitles(page: Page): Promise<string[]> {
  return page.getByTestId("table-row").locator(".row-title").allInnerTexts();
}

test("画面を読み込むと利用記録に web_open が 1 行足される", async ({ page }, testInfo) => {
  const before = webOpenCount();
  const webOpen = page.waitForResponse(
    (response) => response.url().endsWith("/api/usage/web-open") && response.request().method() === "POST",
  );
  await page.goto("/");
  expect((await webOpen).status()).toBe(204);
  expect(webOpenCount()).toBe(before + 1);

  const navigation = page.getByRole("navigation");
  for (const database of ["feature", "item", "project", "run"]) {
    await expect(navigation.getByRole("link", { name: database, exact: true })).toBeVisible();
  }
  await page.screenshot({ path: testInfo.outputPath("home.png"), fullPage: true });
});

test("詳細ページでプロパティと本文を編集し、選択肢を足して保存すると、読み込み直しても残る", async ({
  page,
}, testInfo) => {
  await page.goto("/db/item");
  await page.getByRole("link", { name: "一覧の並び", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("一覧の並び");
  // 取り込んだ run が、この item を参照している行として並ぶ
  await expect(page.getByTestId("backlink")).toHaveCount(1);

  await page.getByLabel("expected", { exact: true }).fill("ノートが更新日時の新しい順に並び、ピン留めは先頭に残る");
  const labels = page.getByRole("group", { name: "labels" });
  await labels.getByLabel("labels に足す選択肢").fill("課金");
  await labels.getByRole("button", { name: "足す" }).click();
  await expect(labels.getByRole("checkbox", { name: "課金" })).toBeChecked();
  await page.getByRole("button", { name: "本文を編集" }).click();
  await page.getByLabel("本文", { exact: true }).fill("## 確認の順\n\n1. ノートを 3 つ作る\n2. 1 つ目を編集する");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByRole("status")).toHaveText("保存しました");

  await page.reload();
  await expect(page.getByLabel("expected", { exact: true })).toHaveValue(
    "ノートが更新日時の新しい順に並び、ピン留めは先頭に残る",
  );
  await expect(page.getByRole("group", { name: "labels" }).getByRole("checkbox", { name: "課金" })).toBeChecked();
  await expect(page.getByTestId("row-body").getByRole("heading", { name: "確認の順" })).toBeVisible();
  await expect(page.getByTestId("row-body").getByRole("listitem")).toHaveText([
    "ノートを 3 つ作る",
    "1 つ目を編集する",
  ]);
  await page.screenshot({ path: testInfo.outputPath("row-detail.png"), fullPage: true });
});

test("表ビューで label・自動化・テキストで絞り込み、列で並べ替える", async ({ page }, testInfo) => {
  await page.goto("/db/item");
  const rows = page.getByTestId("table-row");
  // fixture の項目: ルート 2 + note_list 5 + note_editor 3 + settings 2
  await expect(rows).toHaveCount(12);
  await expect(page.getByTestId("row-count")).toHaveText("12 行");

  await page.getByLabel("labels", { exact: true }).selectOption("課金");
  await expect(rows).toHaveCount(1);
  expect(await rowTitles(page)).toEqual(["一覧の並び"]);
  await expect(page.getByTestId("row-count")).toHaveText("1 / 12 行");
  await page.screenshot({ path: testInfo.outputPath("table-filter-label.png"), fullPage: true });

  await page.getByRole("button", { name: "絞り込みを消す" }).click();
  await expect(rows).toHaveCount(12);
  await page.getByLabel("automation", { exact: true }).selectOption("manual");
  expect((await rowTitles(page)).sort()).toEqual(["検索で絞り込める", "画像を貼れる", "音声入力で書ける"].sort());

  await page.getByRole("button", { name: "絞り込みを消す" }).click();
  await page.getByLabel("テキスト", { exact: true }).fill("テーマ");
  expect(await rowTitles(page)).toEqual(["テーマを切り替えられる"]);
  await page.getByRole("button", { name: "絞り込みを消す" }).click();
  await expect(rows).toHaveCount(12);

  const nameHeader = page.getByRole("columnheader", { name: "name" });
  await nameHeader.getByRole("button").click();
  await expect(nameHeader).toHaveAttribute("aria-sort", "ascending");
  const ascending = await rowTitles(page);
  await nameHeader.getByRole("button").click();
  await expect(nameHeader).toHaveAttribute("aria-sort", "descending");
  expect(await rowTitles(page)).toEqual([...ascending].reverse());
  await page.screenshot({ path: testInfo.outputPath("table-sort.png"), fullPage: true });
});

test("QA の入力で OK / NG / あとで を記録するとフッターの件数が変わり、全部コピーが未入力と NG を markdown にする", async ({
  page,
  context,
}, testInfo) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/qa");
  await page.getByLabel("project", { exact: true }).selectOption("sample-notes");
  await page.getByLabel("feature", { exact: true }).selectOption("note_list");
  await expect(page).toHaveURL(/\/qa\?repo=sample-notes&feature=note_list$/);
  // 記録する commit は project の checkout (fixtures/qa/sample-notes を含むこのリポジトリ) の HEAD
  await expect(page.getByTestId("head-commit")).toHaveText(/^commit [0-9a-f]{7}$/);
  const entered = page.getByTestId("qa-entered");
  // 取り込んだ run は fixture の commit で記録されていて、今の HEAD では未入力
  await expect(entered).toHaveText("0 / 5 件 入力済み");
  await page.getByLabel("検証環境").fill("Chromium (E2E)");

  const currentName = page.getByTestId("qa-current").getByRole("heading", { level: 2 });
  const record = async (name: string, result: "OK" | "NG" | "あとで", note?: string) => {
    await expect(currentName).toHaveText(name);
    await page.getByRole("button", { name: result, exact: true }).click();
    if (note !== undefined) {
      await page.getByLabel("メモ").fill(note);
    }
    await page.getByRole("button", { name: "記録して次へ →" }).click();
  };
  await record("一覧の並び", "OK");
  await expect(entered).toHaveText("1 / 5 件 入力済み");
  await expect(page.getByTestId("qa-ok")).toHaveText("OK 1");
  await record("検索で絞り込める", "NG", "1 文字目では絞り込まれない");
  await expect(entered).toHaveText("2 / 5 件 入力済み");
  await expect(page.getByTestId("qa-ng")).toHaveText("NG 1");
  await record("空の案内", "あとで");
  await expect(entered).toHaveText("3 / 5 件 入力済み");
  await expect(page.getByTestId("qa-ok")).toHaveText("OK 1");
  await expect(page.getByTestId("qa-ng")).toHaveText("NG 1");
  await expect(page.getByTestId("qa-later")).toHaveText("あとで 1");
  await expect(currentName).toHaveText("ピン留めの固定");
  await page.screenshot({ path: testInfo.outputPath("qa-input.png"), fullPage: true });

  await page.getByRole("button", { name: "全部コピー" }).click();
  await expect(page.getByRole("status")).toHaveText("未入力と NG の項目をコピーしました");
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied).toMatch(/^## QA の残り: sample-notes \/ note_list \(commit [0-9a-f]{7}\)/);
  expect(copied).toContain(
    "### 未入力 (2 件)\n\n- [ ] 4. **ピン留めの固定**: ピン留めしたノートが先頭に固定される\n- [ ] 5. **スワイプで削除**: 一覧の行を左にスワイプすると削除ボタンが出る",
  );
  expect(copied).toContain(
    "### NG (1 件)\n\n- [ ] 2. **検索で絞り込める**: 検索語を入れると、含むノートだけが残る\n  - メモ: 1 文字目では絞り込まれない",
  );
  expect(copied).not.toContain("一覧の並び");
  expect(copied).not.toContain("空の案内");

  // 画面から記録した run は、検証環境と HEAD の commit を持つ run として DB に入っている
  const response = await page.request.get("/api/databases/run/rows");
  const { rows } = (await response.json()) as {
    rows: { values: Record<string, { value: unknown } | undefined> }[];
  };
  const recorded = rows.filter((row) => row.values.source?.value === "record");
  expect(recorded.map((row) => row.values.result?.value)).toEqual(["ok", "ng", "later"]);
  for (const row of recorded) {
    expect(row.values.env?.value).toBe("Chromium (E2E)");
    expect(row.values.commit?.value).toMatch(/^[0-9a-f]{40}$/);
  }

  // 同じ LAN の iPhone から開いた時の幅
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: testInfo.outputPath("qa-input-phone.png"), fullPage: true });
});
