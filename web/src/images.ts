/**
 * url 型の値 `url` が、画面がサムネイルとして読み込む画像の URL か。http(s) で、パス (クエリと `#` の後ろを除く) が
 * png / jpg / jpeg / gif / webp の拡張子 (大文字小文字を問わない) で終わる時に true。
 */
export function isImageUrl(url: string): boolean {
  if (!/^https?:\/\//i.test(url)) {
    return false;
  }
  try {
    return /\.(png|jpe?g|gif|webp)$/i.test(new URL(url).pathname);
  } catch {
    return false;
  }
}
