import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, type Locator, type Page, type Request, test } from "@playwright/test";
import { tansuHome, tansuToken } from "../paths.ts";

// fixtures/qa/sample-notes を取り込んだ 1 つの DB を、上のテストから順に書き換えながら確かめる
// (詳細ページで足した label を、表ビューの絞り込みが使う)。
test.describe.configure({ mode: "serial" });

/** 画面が読み込む行の画像の代わりに返す、縦長のスマホのスクリーンショットに見立てた画像。 */
const PLACEHOLDER_SCREENSHOT =
  '<svg xmlns="http://www.w3.org/2000/svg" width="180" height="390"><rect width="180" height="390" fill="#dfe6ee"/><rect x="12" y="24" width="156" height="20" rx="4" fill="#9fb0c3"/><rect x="12" y="60" width="156" height="300" rx="8" fill="#ffffff"/></svg>';

// 画面は行の画像の URL (fixture は https://example.com/...) をサムネイルとして読み込む。CI から外部へ取りに行かないよう、
// すべてのテストで (新しいタブを含めて) 差し替える
test.beforeEach(async ({ context }) => {
  await context.route("https://example.com/**", (route) =>
    route.fulfill({ status: 200, contentType: "image/svg+xml", body: PLACEHOLDER_SCREENSHOT }),
  );
});

/**
 * tansu serve が表示する URL と同じく `#token=` を付けた `pagePath`。テストごとにブラウザの localStorage が空になるため、
 * 各テストの最初の画面はこれで開いて画面にトークンを渡す (同じテストの中の 2 回目からは付けなくてよい)。
 */
function withToken(pagePath: string): string {
  return `${pagePath}#token=${tansuToken()}`;
}

/** テストから API を直接呼ぶ時に付けるトークンのヘッダー。 */
function apiHeaders(): Record<string, string> {
  return { Authorization: `Bearer ${tansuToken()}` };
}

test("トークンを受け取っていない画面は API が 401 になり、開き直す案内を出す", async ({ page }, testInfo) => {
  const databases = page.waitForResponse((response) => response.url().endsWith("/api/databases"));
  await page.goto("/");
  expect((await databases).status()).toBe(401);
  await expect(page.getByRole("alert")).toContainText("#token=");
  await page.screenshot({ path: testInfo.outputPath("unauthorized.png"), fullPage: true });

  // 案内どおり同じタブで #token= 付きの URL を開くと (fragment だけの遷移)、読み込み直してトークンで API を呼べる
  const authorized = page.waitForResponse(
    (response) => response.url().endsWith("/api/databases") && response.status() === 200,
  );
  await page.goto(withToken("/"));
  await authorized;
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page).toHaveURL(/\/$/);
});

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
  await page.goto(withToken("/"));
  expect((await webOpen).status()).toBe(204);
  expect(webOpenCount()).toBe(before + 1);
  // 受け取ったトークンはアドレスバーと履歴に残さない
  await expect(page).toHaveURL(/\/$/);

  const navigation = page.getByRole("navigation");
  for (const database of ["feature", "item", "project", "run"]) {
    await expect(navigation.getByRole("link", { name: database, exact: true })).toBeVisible();
  }
  await page.screenshot({ path: testInfo.outputPath("home.png"), fullPage: true });
});

/** item の行 `name` の行 ID (API から読む)。 */
async function itemRowId(page: Page, name: string): Promise<string> {
  const response = await page.request.get("/api/databases/item/rows", { headers: apiHeaders() });
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
  await page.goto(withToken("/db/item"));
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
  await page.goto(withToken("/db/item"));
  const rows = page.getByTestId("table-row");
  // fixture の項目: ルート 2 + note_list 5 + note_editor 3 + settings 2
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
    headers: apiHeaders(),
    data: { add: ["points:number", "verified:checkbox", "reference:url", "due:date"] },
  });
  expect(schema.ok()).toBe(true);
  const written = await page.request.post("/api/databases/item/rows", {
    headers: apiHeaders(),
    data: {
      id: await itemRowId(page, "一覧の並び"),
      values: { points: 3, verified: true, reference: "https://example.com/spec/note-list", due: "2026-10-20" },
    },
  });
  expect(written.ok()).toBe(true);

  // 足した列まで 1 枚のスクリーンショットに収める幅
  await page.setViewportSize({ width: 1800, height: 900 });
  await page.goto(withToken("/db/item"));
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

  // 数として保存できない入力 (Chromium では有限でない 1e999 は badInput で value が空になる) は保存せずに取り消し、
  // 保存済みの値を消さない
  await page.getByRole("button", { name: "一覧の並び の points を編集" }).click();
  const pointsEditor = page.getByLabel("一覧の並び の points", { exact: true });
  await pointsEditor.fill("1e999");
  await pointsEditor.press("Enter");
  await expect(pointsEditor).toHaveCount(0);
  const rowsResponse = await page.request.get("/api/databases/item/rows", { headers: apiHeaders() });
  const { rows: itemRows } = (await rowsResponse.json()) as {
    rows: { values: { name?: { value: string }; points?: { value: number } } }[];
  };
  expect(itemRows.find((candidate) => candidate.values.name?.value === "一覧の並び")?.values.points?.value).toBe(3);
});

test("表のセルをその場で編集し、checkbox をクリックで切り替えると、読み込み直しても残る", async ({
  page,
}, testInfo) => {
  await page.goto(withToken("/db/item"));
  const expected = "ライト・ダーク・端末に合わせるの 3 つから選べる";
  await page.getByRole("button", { name: "テーマを切り替えられる の expected を編集" }).click();
  const editor = page.getByLabel("テーマを切り替えられる の expected", { exact: true });
  await expect(editor).toBeFocused();
  await editor.fill(expected);
  await page.screenshot({ path: testInfo.outputPath("table-cell-editing.png"), fullPage: true });

  // 保存の応答を待つ間はほかのセルの編集を始められず、保存に失敗しても編集欄と入力は残り、そのまま送り直せる
  // 応答を待つ間の画面を確かめ終わるまで、失敗の応答を返さずに止めておく
  let releaseFailure: () => void = () => undefined;
  const failureReleased = new Promise<void>((resolve) => {
    releaseFailure = resolve;
  });
  await page.route("**/api/databases/item/rows", async (route) => {
    if (route.request().method() !== "POST") {
      await route.continue();
      return;
    }
    await failureReleased;
    await route.fulfill({ status: 500, contentType: "application/json", body: '{"error":"E2E で失敗させた"}' });
  });
  const failed = itemRowWritten(page);
  await editor.press("Enter");
  await expect(page.getByRole("button", { name: "言語を切り替えられる の expected を編集" })).toBeDisabled();
  await expect(page.getByRole("checkbox", { name: "言語を切り替えられる の verified" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "+ 新規", exact: true })).toBeDisabled();
  releaseFailure();
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

  // multi_select は選択肢の文字を押して変え、「完了」か編集欄の外を押すと保存する
  const themeRow = page
    .getByTestId("table-row")
    .filter({ has: page.getByRole("button", { name: "テーマを切り替えられる を開く" }) });
  await page.getByRole("button", { name: "テーマを切り替えられる の spec_ids を編集" }).click();
  const specIdsEditor = page.getByRole("group", { name: "テーマを切り替えられる の spec_ids" });
  await specIdsEditor.getByText("S3", { exact: true }).click();
  await specIdsEditor.getByText("S5", { exact: true }).click();
  await expect(specIdsEditor).toBeVisible();
  const savedChoices = itemRowWritten(page);
  await specIdsEditor.getByRole("button", { name: "完了" }).click();
  expect((await savedChoices).ok()).toBe(true);
  await expect(themeRow.locator(".tag", { hasText: /^S3$/ })).toBeVisible();
  await expect(themeRow.locator(".tag", { hasText: /^S5$/ })).toBeVisible();
  await page.getByRole("button", { name: "テーマを切り替えられる の spec_ids を編集" }).click();
  await specIdsEditor.getByText("S5", { exact: true }).click();
  const savedOutside = itemRowWritten(page);
  await page.getByRole("heading", { level: 1 }).click();
  expect((await savedOutside).ok()).toBe(true);
  await expect(specIdsEditor).toHaveCount(0);
  await expect(themeRow.locator(".tag", { hasText: /^S5$/ })).toHaveCount(0);
  await expect(themeRow.locator(".tag", { hasText: /^S3$/ })).toBeVisible();
  // 選択肢を押した後の Esc は保存せずに取り消す
  await page.getByRole("button", { name: "テーマを切り替えられる の spec_ids を編集" }).click();
  await specIdsEditor.getByText("S1", { exact: true }).click();
  await page.keyboard.press("Escape");
  await expect(specIdsEditor).toHaveCount(0);
  await expect(themeRow.locator(".tag", { hasText: /^S1$/ })).toHaveCount(0);
  // キーボードで編集欄の外へ移っても保存する
  await page.getByRole("button", { name: "テーマを切り替えられる の spec_ids を編集" }).click();
  await specIdsEditor.getByText("S2", { exact: true }).click();
  const savedByTab = itemRowWritten(page);
  await specIdsEditor.getByRole("button", { name: "完了" }).focus();
  await page.keyboard.press("Tab");
  expect((await savedByTab).ok()).toBe(true);
  await expect(specIdsEditor).toHaveCount(0);
  await expect(themeRow.locator(".tag", { hasText: /^S2$/ })).toBeVisible();

  await page.reload();
  await expect(page.getByTestId("table-row").getByText(expected)).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "テーマを切り替えられる の verified" })).toBeChecked();
  await page.screenshot({ path: testInfo.outputPath("table-cell-edited.png"), fullPage: true });
});

test("表の最後の「+ 新規」で空の行が足され、絞り込み中でもその場で名前を付けられる", async ({ page }, testInfo) => {
  await page.goto(withToken("/db/item"));
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
  await page.goto(withToken("/db/item"));
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
  // パネルの中をスクロールして、本文と参照している run の履歴もスクリーンショットに残す
  const peekBody = peek.getByTestId("row-body");
  await peekBody.scrollIntoViewIfNeeded();
  await expect(peekBody).toBeInViewport();
  await page.screenshot({ path: testInfo.outputPath("peek-body.png") });
  const peekBacklink = peek.getByTestId("backlink");
  await peekBacklink.scrollIntoViewIfNeeded();
  await expect(peekBacklink).toBeInViewport();
  await page.screenshot({ path: testInfo.outputPath("peek-backlinks.png") });
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

  // パネルで保存した後の表の読み直しの間は、表の編集と「+ 新規」を止める。保存の後の item の行の取得は、
  // 1 回目がパネル自身の読み直し、2 回目が表の読み直しで、2 回目の応答を確かめ終わるまで止めておく
  let itemRowsFetches = 0;
  let releaseTableReload: () => void = () => undefined;
  const tableReloadReleased = new Promise<void>((resolve) => {
    releaseTableReload = resolve;
  });
  await page.route("**/api/databases/item/rows", async (route) => {
    if (route.request().method() === "GET") {
      itemRowsFetches += 1;
      if (itemRowsFetches === 2) {
        await tableReloadReleased;
      }
    }
    await route.continue();
  });
  await peek.getByRole("button", { name: "保存", exact: true }).click();
  const newRowButton = page.getByRole("button", { name: "+ 新規", exact: true });
  await expect(newRowButton).toBeDisabled();
  await expect(page.getByRole("button", { name: "検索で絞り込める の expected を編集" })).toBeDisabled();
  releaseTableReload();
  await expect(newRowButton).toBeEnabled();
  await page.unroute("**/api/databases/item/rows");
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

  // 無い行の URL で開いても、スマホ幅の全画面のパネルを閉じて表に戻れる
  await page.goto("/db/item?peek=missing-row-id");
  await expect(peek.getByRole("alert")).toContainText("missing-row-id");
  await peek.getByRole("button", { name: "閉じる" }).click();
  await expect(peek).toHaveCount(0);
  await expect(page).toHaveURL(/\/db\/item$/);
});

/** サムネイルの `img` が読み込まれて描かれるまで待つ (`loading="lazy"` のため、呼ぶ前に画面に入れておく)。 */
async function expectThumbnailLoaded(image: Locator) {
  await image.scrollIntoViewIfNeeded();
  await expect.poll(() => image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBeGreaterThan(0);
}

test("行の詳細と関連先のカードで画像の URL がサムネイルで出て、押すと元の URL を新しいタブで開き、表のセルは変わらない", async ({
  page,
  context,
}, testInfo) => {
  // fixture の note_list の「一覧の並び」のエビデンスの最初の画像を、取り込んだ run が image_url に持つ
  const imageUrl = "https://example.com/sample-notes/note_list/order.png";
  const imageRequests: Request[] = [];
  page.on("request", (request) => {
    if (request.resourceType() === "image") {
      imageRequests.push(request);
    }
  });
  await page.goto(withToken("/db/item"));
  await page.getByRole("button", { name: "一覧の並び を開く" }).click();
  const backlink = page.getByTestId("peek").getByTestId("backlink");
  await expect(backlink).toHaveCount(1);
  // カードは URL のリンクを残し、その下にサムネイルを出す
  await expect(backlink.getByRole("link", { name: imageUrl, exact: true })).toBeVisible();
  const cardThumbnail = backlink.getByRole("img", { name: "image_url の画像" });
  await expectThumbnailLoaded(cardThumbnail);
  expect((await cardThumbnail.boundingBox())?.height).toBeLessThanOrEqual(160);
  await page.screenshot({ path: testInfo.outputPath("peek-backlink-thumbnail.png") });
  // 画像の取得は Referer を付けない (documents/PROJECT.md「制約」の例外の条件)
  const cardImageRequest = imageRequests.find((request) => request.url() === imageUrl);
  expect(cardImageRequest).toBeDefined();
  expect((await cardImageRequest?.allHeaders())?.referer).toBeUndefined();

  const popupOpened = page.waitForEvent("popup");
  await cardThumbnail.click();
  const popup = await popupOpened;
  await popup.waitForLoadState();
  expect(popup.url()).toBe(imageUrl);
  await popup.close();

  // 読み込めない本文の画像はリンクになる (後に登録した route が先に当たる)
  const missingImageUrl = "https://example.com/e2e/missing.png";
  await context.route(missingImageUrl, (route) => route.fulfill({ status: 404, body: "" }));
  await backlink.getByRole("link").first().click();
  await expect(page).toHaveURL(/\/db\/run\/[^/?]+$/);
  const imageUrlInput = page.getByLabel("image_url", { exact: true });
  await expect(imageUrlInput).toHaveValue(imageUrl);
  const propertyThumbnail = page.getByRole("img", { name: "image_url の画像" });
  await expectThumbnailLoaded(propertyThumbnail);
  // 入力の途中の URL は読み込まず、保存済みの値の画像を出し続ける
  const draftImageUrl = "https://example.com/e2e/draft.png";
  await imageUrlInput.fill(draftImageUrl);
  await expect(propertyThumbnail).toHaveAttribute("src", imageUrl);
  expect(imageRequests.filter((request) => request.url() === draftImageUrl)).toHaveLength(0);
  await imageUrlInput.fill(imageUrl);
  // 取り込んだ run の本文はエビデンスの画像を markdown の画像で持つ
  const rowBody = page.getByTestId("row-body");
  await expectThumbnailLoaded(rowBody.getByRole("img", { name: imageUrl }));

  await page.getByRole("button", { name: "本文を編集" }).click();
  const bodyEditor = page.getByLabel("本文", { exact: true });
  await bodyEditor.fill(`${await bodyEditor.inputValue()}\n\n![消えた画像](${missingImageUrl})`);
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByRole("status")).toHaveText("保存しました");
  // 読み込みの失敗の前はサムネイルのリンクも同じ名前を持つため、img が消えて (壊れた画像を出さず) から、残ったリンクを確かめる
  // (読み込みを始めさせるため、失敗の前後のどちらでも 1 つだけあるリンクを画面に入れる)
  const missingImageLink = rowBody.getByRole("link", { name: "消えた画像" });
  await missingImageLink.scrollIntoViewIfNeeded();
  await expect(rowBody.getByRole("img", { name: "消えた画像" })).toHaveCount(0);
  await expect(missingImageLink).toHaveAttribute("href", missingImageUrl);
  await expect(rowBody.locator(".thumbnail")).toHaveCount(1);
  await expectThumbnailLoaded(rowBody.getByRole("img", { name: imageUrl }));
  await page.screenshot({ path: testInfo.outputPath("row-detail-thumbnail.png"), fullPage: true });

  await page.goto("/db/run");
  await expect(page.getByTestId("table-row").getByRole("link", { name: imageUrl, exact: true })).toBeVisible();
  await expect(page.getByTestId("table-row").locator("img")).toHaveCount(0);
});

test("QA の入力で OK / NG / あとで を記録するとフッターの件数が変わり、全部コピーが未入力と NG を markdown にする", async ({
  page,
  context,
}, testInfo) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto(withToken("/qa"));
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
  const response = await page.request.get("/api/databases/run/rows", { headers: apiHeaders() });
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
