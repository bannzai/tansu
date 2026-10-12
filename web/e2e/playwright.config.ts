import { defineConfig, devices } from "@playwright/test";
import { port, tansuHome } from "./paths.ts";

export default defineConfig({
  testDir: "./tests",
  // CI の e2e job がこのディレクトリを e2e-screenshots の artifact として上げる (.github/workflows/ci.yml)
  outputDir: "../test-results",
  forbidOnly: Boolean(process.env.CI),
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  // テストは 1 つの DB と利用記録を共有して書き込むため、並列に動かさない
  workers: 1,
  fullyParallel: false,
  use: {
    ...devices["Desktop Chrome"],
    baseURL: `http://127.0.0.1:${port}`,
  },
  webServer: {
    // fixtures/ の合成 QA.md を取り込んだサーバー (.claude/rules/synthetic-fixtures.md)。作業ディレクトリは web/
    command: "bash e2e/start-server.sh",
    cwd: "..",
    url: `http://127.0.0.1:${port}/api/databases`,
    reuseExistingServer: false,
    env: {
      TANSU_HOME: tansuHome,
      TANSU_PORT: String(port),
      // CI の e2e job が web/dist を埋め込んでビルドした tansu
      TANSU_BIN: process.env.TANSU_BIN ?? "../target/release/tansu",
    },
  },
});
