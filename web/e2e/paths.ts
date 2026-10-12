import os from "node:os";
import path from "node:path";

/**
 * E2E のサーバーの `TANSU_HOME` (DB と利用記録 usage.jsonl の置き場所)。runner のホームディレクトリに書かないよう
 * 一時ディレクトリの下に置く。設定は Playwright の本体と worker が別々に読み、テストも利用記録を読むため、
 * どこで読んでも同じパスになる固定の名前にする。
 */
export const tansuHome = path.join(os.tmpdir(), "tansu-e2e-home");

/** E2E のサーバーが待ち受けるポート。tansu serve の既定 (crates/server/src/lib.rs の DEFAULT_PORT) で、CI の runner では空いている。 */
export const port = 7979;
