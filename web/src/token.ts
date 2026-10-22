// HTTP API のトークン (crates/core/src/api_token.rs)。tansu serve が表示する URL の `#token=<トークン>` で受け取り、
// この origin (ホストとポート) の localStorage に持つ。API の要求には `Authorization: Bearer <トークン>` で付ける (api.ts)。

/** トークンを持つ localStorage のキー。 */
const TOKEN_STORAGE_KEY = "tansu-token";

/** このページで受け取ったトークン。localStorage を使えない時 (Safari のプライベートブラウズ等) もこのページの間は使える。 */
let pageToken: string | null = null;

/** URL の fragment `hash` (`#token=<トークン>`) のトークン。無い・空なら null。 */
export function tokenFromHash(hash: string): string | null {
  const token = new URLSearchParams(hash.replace(/^#/, "")).get("token");
  return token === "" ? null : token;
}

/**
 * URL に `#token=` があれば、そのトークンを持ち、URL から fragment を消す (アドレスバーと履歴にトークンを残さない)。
 * 画面の描画と API の呼び出しより前に 1 回呼ぶ。
 */
export function takeTokenFromUrl(): void {
  const token = tokenFromHash(window.location.hash);
  if (token === null) {
    return;
  }
  pageToken = token;
  try {
    localStorage.setItem(TOKEN_STORAGE_KEY, token);
  } catch (error: unknown) {
    console.warn("トークンを localStorage に保存できませんでした (このページの間だけ使います)", error);
  }
  window.history.replaceState(window.history.state, "", `${window.location.pathname}${window.location.search}`);
}

/** API に送るトークン。受け取っていなければ null。 */
export function storedToken(): string | null {
  if (pageToken !== null) {
    return pageToken;
  }
  try {
    return localStorage.getItem(TOKEN_STORAGE_KEY);
  } catch {
    return null;
  }
}
