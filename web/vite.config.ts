import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    // vitest の既定は *.spec.ts も拾うため、Playwright の E2E (e2e/) を単体テストとして動かさないよう src だけにする
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
