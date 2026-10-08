import { describe, expect, test } from "bun:test";
import { run } from "./cli";
import { version } from "./version";

describe("tansu CLI", () => {
  test("引数なしと --version はバージョンを出して exit 0", () => {
    expect(run([])).toEqual({ stdout: `tansu ${version}\n`, stderr: "", exitCode: 0 });
    expect(run(["--version"])).toEqual({ stdout: `tansu ${version}\n`, stderr: "", exitCode: 0 });
  });

  test("未対応のサブコマンドは stderr に理由を出して exit 2 (stdout は空)", () => {
    const result = run(["nope"]);
    expect(result.exitCode).toBe(2);
    expect(result.stdout).toBe("");
    expect(result.stderr).toContain("nope");
  });
});
