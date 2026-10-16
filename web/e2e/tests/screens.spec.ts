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

/** item の行 `name` の行 ID (API から読む)。 */
async function itemRowId(page: Page, name: string): Promise<string> {
  const response = await page.request.get("/api/databases/item/rows");
  const { rows } = (await response.json()) as { rows: { id: string; values: { name?: { value: string } } }[] };
  const row = rows.find((candidate) => candidate.values.name?.value === name);
  if (row === undefined) {
    throw new Error(`item に ${name} がありません`);
  }
  return row.id;
}

/** 次の item の行の書き込み (POST /api/databases/item/rows) の応答を待つ。 */
function itemRowWritten(page: Page) {
  return page.waitForResponse(
    (response) => response.url().endsWith("/api/databases/item/rows") && response.request().method() === "POST",
  );
}

test("詳細ページでプロパティと本文を編集し、選択肢を足して保存すると、読み込み直しても残る", async ({
  page,
}, testInfo) => {
  await page.goto("/db/item");
  await page.getByRole("button", { name: "一覧の並び を開く" }).click();
  await page.getByTestId("peek").getByRole("link", { name: "全画面で開く" }).click();
  await expect(page).toHaveURL(/\/db\/item\/[^/?]+$/);
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

test("表ビューのツールバーで label・自動化・テキストで絞り込み、列で並べ替える", async ({ page }, testInfo) => {
  await page.goto("/db/item");
  const rows = page.getByTestId("table-row");
  // fixture の項目: ルート 2 + note_list 5 + note_editor 3 + settings 2 (同じ名前の 2 つ目は読み飛ばす)
  await expect(rows).toHaveCount(12);
  await expect(page.getByTestId("row-count")).toHaveText("12 行");

  await page.getByRole("button", { name: "フィルター", exact: true }).click();
  await page.getByLabel("labels", { exact: true }).selectOption("課金");
  await expect(rows).toHaveCount(1);
  expect(await rowTitles(page)).toEqual(["一覧の並び"]);
  await expect(page.getByTestId("row-count")).toHaveText("1 / 12 行");
  // ツールバーのボタンに、効いている絞り込みの数が出る
  await expect(page.getByRole("button", { name: "フィルター 1", exact: true })).toBeVisible();
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

  await page.getByRole("button", { name: "並べ替え", exact: true }).click();
  const nameHeader = page.getByRole("columnheader", { name: "name", exact: true });
  await page.getByLabel("並べ替えるプロパティ", { exact: true }).selectOption("name");
  await expect(nameHeader).toHaveAttribute("aria-sort", "ascending");
  const ascending = await rowTitles(page);
  await page.getByLabel("並べ替えの向き", { exact: true }).selectOption("desc");
  await expect(nameHeader).toHaveAttribute("aria-sort", "descending");
  expect(await rowTitles(page)).toEqual([...ascending].reverse());
  await page.screenshot({ path: testInfo.outputPath("table-sort.png"), fullPage: true });
});

test("表の列見出しに型のアイコンが出て、セルが型に合わせて描かれる", async ({ page }, testInfo) => {
  // QA 層の表には number・checkbox・url・date のプロパティが無いため、item に足して値を書く
  const schema = await page.request.post("/api/databases/item/schema", {
    data: { add: ["points:number", "verified:checkbox", "reference:url", "due:date"] },
  });
  expect(schema.ok()).toBe(true);
  const written = await page.request.post("/api/databases/item/rows", {
    data: {
      id: await itemRowId(page, "一覧の並び"),
      values: { points: 3, verified: true, reference: "https://example.com/spec/note-list", due: "2026-10-20" },
    },
  });
  expect(written.ok()).toBe(true);

  // 足した列まで 1 枚のスクリーンショットに収める幅
  await page.setViewportSize({ width: 1800, height: 900 });
  await page.goto("/db/item");
  for (const type of ["text", "number", "select", "multi_select", "checkbox", "url", "date", "relation"]) {
    await expect(page.locator(`th[data-property-type="${type}"] .property-icon`).first()).toBeVisible();
  }
  const row = page.getByTestId("table-row").filter({ has: page.getByRole("button", { name: "一覧の並び を開く" }) });
  await expect(row.locator(".tag", { hasText: /^auto$/ })).toBeVisible();
  await expect(row.locator(".tag", { hasText: "課金" })).toBeVisible();
  await expect(row.getByRole("checkbox", { name: "一覧の並び の verified" })).toBeChecked();
  await expect(row.getByRole("link", { name: "https://example.com/spec/note-list" })).toBeVisible();
  await expect(row.locator('time[datetime="2026-10-20"]')).toHaveText("2026-10-20");
  await expect(row.getByRole("link", { name: "note_list", exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("table-types.png"), fullPage: true });
});

test("表のセルをその場で編集し、checkbox をクリックで切り替えると、読み込み直しても残る", async ({
  page,
}, testInfo) => {
  await page.goto("/db/item");
  const expected = "ライト・ダーク・端末に合わせるの 3 つから選べる";
  await page.getByRole("button", { name: "テーマを切り替えられる の expected を編集" }).click();
  const editor = page.getByLabel("テーマを切り替えられる の expected", { exact: true });
  await expect(editor).toBeFocused();
  await editor.fill(expected);
  await page.screenshot({ path: testInfo.outputPath("table-cell-editing.png"), fullPage: true });

  // 保存の応答を待つ間はほかのセルの編集を始められず、保存に失敗しても編集欄と入力は残り、そのまま送り直せる
  await page.route("**/api/databases/item/rows", async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    // 応答を待つ間の画面を確かめるために遅らせる
    await new Promise((resolve) => setTimeout(resolve, 1000));
    await route.fulfill({ status: 500, contentType: "application/json", body: '{"error":"E2E で失敗させた"}' });
  });
  const failed = itemRowWritten(page);
  await editor.press("Enter");
  await expect(page.getByRole("button", { name: "言語を切り替えられる の expected を編集" })).toBeDisabled();
  await expect(page.getByRole("checkbox", { name: "言語を切り替えられる の verified" })).toBeDisabled();
  expect((await failed).status()).toBe(500);
  await expect(page.getByRole("alert")).toHaveText("E2E で失敗させた");
  await expect(editor).toHaveValue(expected);
  await expect(editor).toBeEnabled();
  await page.unroute("**/api/databases/item/rows");

  const savedText = itemRowWritten(page);
  await editor.press("Enter");
  expect((await savedText).ok()).toBe(true);
  await expect(editor).toHaveCount(0);
  await expect(page.getByTestId("table-row").getByText(expected)).toBeVisible();

  const checkbox = page.getByRole("checkbox", { name: "テーマを切り替えられる の verified" });
  await expect(checkbox).not.toBeChecked();
  const savedCheckbox = itemRowWritten(page);
  await checkbox.click();
  expect((await savedCheckbox).ok()).toBe(true);
  await expect(checkbox).toBeChecked();

  await page.reload();
  await expect(page.getByTestId("table-row").getByText(expected)).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "テーマを切り替えられる の verified" })).toBeChecked();
  await page.screenshot({ path: testInfo.outputPath("table-cell-edited.png"), fullPage: true });
});

test("表の最後の「+ 新規」で空の行が足され、絞り込み中でもその場で名前を付けられる", async ({ page }, testInfo) => {
  await page.goto("/db/item");
  const rows = page.getByTestId("table-row");
  await expect(rows).toHaveCount(12);
  // 足す空の行は labels に「課金」を持たず絞り込みに合わないが、名前を入れ終わるまでは表に残る
  await page.getByRole("button", { name: "フィルター", exact: true }).click();
  await page.getByLabel("labels", { exact: true }).selectOption("課金");
  await expect(rows).toHaveCount(1);
  const created = itemRowWritten(page);
  await page.getByRole("button", { name: "+ 新規", exact: true }).click();
  expect((await created).ok()).toBe(true);
  await expect(rows).toHaveCount(2);
  await expect(page.getByTestId("row-count")).toHaveText("1 / 13 行");

  // 足した行は名前の編集欄が開いた状態になる
  const nameEditor = page.getByRole("textbox", { name: / の name$/ });
  await expect(nameEditor).toBeFocused();
  await nameEditor.fill("E2E で足した項目");
  const named = itemRowWritten(page);
  await nameEditor.press("Enter");
  expect((await named).ok()).toBe(true);
  await expect(rows).toHaveCount(1);

  await page.reload();
  await expect(rows).toHaveCount(13);
  expect(await rowTitles(page)).toContain("E2E で足した項目");
  await page.screenshot({ path: testInfo.outputPath("table-new-row.png"), fullPage: true });
});

test("表の行を開くと表を残したまま右にパネルが開き、切り替え・Esc・URL での復元・スマホ幅の全画面ができる", async ({
  page,
}, testInfo) => {
  await page.goto("/db/item");
  await page.getByRole("button", { name: "一覧の並び を開く" }).click();
  await expect(page).toHaveURL(/\/db\/item\?peek=[^&]+$/);
  const peek = page.getByTestId("peek");
  await expect(peek.getByRole("heading", { level: 1 })).toHaveText("一覧の並び");
  // プロパティ (その場で編集できる)・本文・この item を参照している run
  await expect(peek.getByLabel("expected", { exact: true })).toHaveValue(
    "ノートが更新日時の新しい順に並び、ピン留めは先頭に残る",
  );
  await expect(peek.getByTestId("row-body").getByRole("heading", { name: "確認の順" })).toBeVisible();
  await expect(peek.getByTestId("backlink")).toHaveCount(1);
  // 表は左に残る
  await expect(page.getByTestId("table-row")).toHaveCount(13);
  await expect(page.getByRole("button", { name: "検索で絞り込める を開く" })).toBeInViewport();
  await page.screenshot({ path: testInfo.outputPath("peek.png") });
  const peekUrl = page.url();

  // パネルでまだ保存していない入力は、表のセルで同じ行を書いても残り、表で書いた値はパネルに映る
  const draft = "パネルで書きかけの期待値";
  await peek.getByLabel("expected", { exact: true }).fill(draft);
  const tableCheckbox = page.getByRole("checkbox", { name: "一覧の並び の verified" });
  await expect(tableCheckbox).toBeChecked();
  const written = itemRowWritten(page);
  await tableCheckbox.click();
  expect((await written).ok()).toBe(true);
  await expect(peek.getByLabel("verified", { exact: true })).not.toBeChecked();
  await expect(peek.getByLabel("expected", { exact: true })).toHaveValue(draft);
  // 続けて書くと (更新時刻が秒単位で同じでも) 後の値がパネルに映る
  const writtenAgain = itemRowWritten(page);
  await tableCheckbox.click();
  expect((await writtenAgain).ok()).toBe(true);
  await expect(peek.getByLabel("verified", { exact: true })).toBeChecked();
  // 表のセルの編集を Esc で取り消しても、パネルは閉じず書きかけも残る
  await page.getByRole("button", { name: "一覧の並び の labels を編集" }).click();
  const labelsEditor = page.getByRole("group", { name: "一覧の並び の labels" });
  await expect(labelsEditor).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(labelsEditor).toHaveCount(0);
  await expect(peek.getByLabel("expected", { exact: true })).toHaveValue(draft);

  await page.getByRole("button", { name: "検索で絞り込める を開く" }).click();
  await expect(peek.getByRole("heading", { level: 1 })).toHaveText("検索で絞り込める");
  expect(page.url()).not.toBe(peekUrl);

  await page.keyboard.press("Escape");
  await expect(peek).toHaveCount(0);
  await expect(page).toHaveURL(/\/db\/item$/);

  await page.goto(peekUrl);
  await expect(peek.getByRole("heading", { level: 1 })).toHaveText("一覧の並び");

  // 同じ LAN の iPhone から開いた時の幅
  await page.setViewportSize({ width: 390, height: 844 });
  const box = await peek.boundingBox();
  expect(box?.x).toBe(0);
  expect(box?.width).toBeGreaterThan(360);
  await page.screenshot({ path: testInfo.outputPath("peek-phone.png") });

  await peek.getByRole("button", { name: "閉じる" }).click();
  await expect(peek).toHaveCount(0);
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
