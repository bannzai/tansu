import { expect, test } from "vitest";
import { isImageUrl } from "./images.ts";

test("http(s) でパスが画像の拡張子で終わる URL は、大文字小文字を問わず画像の URL", () => {
  for (const url of [
    "https://example.com/sample-notes/note_list/order.png",
    "http://example.com/a.jpg",
    "https://example.com/a.JPEG",
    "https://example.com/a.Gif",
    "https://example.com/a.webp",
    "HTTPS://example.com/a.png",
    "https://example.com/a.png?v=2",
    "https://example.com/a.png#top",
  ]) {
    expect(isImageUrl(url)).toBe(true);
  }
});

test("画像の拡張子で終わらない URL と http(s) 以外の URL は画像の URL ではない", () => {
  for (const url of [
    "https://example.com/spec/note-list",
    "https://example.com/a.png/view",
    "https://example.com/a.svg",
    "https://example.com/?file=a.png",
    "javascript:alert(1)//a.png",
    "data:image/png;base64,AAAA.png",
    "file:///Users/a.png",
    "a.png",
    "",
  ]) {
    expect(isImageUrl(url)).toBe(false);
  }
});
