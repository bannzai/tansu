import { expect, test } from "vitest";
import { APP_NAME } from "./appName.ts";

test("見出しに出すアプリ名は tansu", () => {
  expect(APP_NAME).toBe("tansu");
});
