import { expect, test } from "vitest";
import { tokenFromHash } from "./token.ts";

test("tansu serve が表示する URL の #token= からトークンを読む", () => {
  expect(tokenFromHash("#token=abc_DEF-123")).toBe("abc_DEF-123");
  expect(tokenFromHash("token=abc")).toBe("abc");
});

test("#token= が無い・空の URL はトークンを持たない", () => {
  expect(tokenFromHash("")).toBeNull();
  expect(tokenFromHash("#")).toBeNull();
  expect(tokenFromHash("#token=")).toBeNull();
  expect(tokenFromHash("#other=abc")).toBeNull();
});
