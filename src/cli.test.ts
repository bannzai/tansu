import { describe, expect, test } from "bun:test";
import { run } from "./cli";
import { version } from "./version";

describe("tansu CLI", () => {
  test("引数なしと --version はバージョンを出して exit 0", () => {
    expect(run([])).toEqual({ stdout: `tansu ${version}\n`, exitCode: 0 });
    expect(run(["--version"])).toEqual({ stdout: `tansu ${version}\n`, exitCode: 0 });
  });

  test("未対応のサブコマンドは exit 2", () => {
    const result = run(["nope"]);
    expect(result.exitCode).toBe(2);
    expect(result.stdout).toContain("nope");
  });
});
