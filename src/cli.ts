#!/usr/bin/env bun
// tansu の CLI の入口。サブコマンド (db / query / upsert / delete / export / qa / serve / mcp) は
// ロードマップの子 issue で足す。立ち上げの時点では version の表示だけを持ち、CI が端から端まで通ることを確かめる。
import { version } from "./version";

/** CLI の 1 回の実行の結果。stdout は機械可読の出力 (`--json` で jq に渡す)、stderr はエラーと案内で、混ぜない */
export type CliResult = { stdout: string; stderr: string; exitCode: number };

/** CLI の引数を受けて、標準出力・標準エラー出力に書く文字列と exit code を返す。I/O を伴わないためテストから直接呼べる */
export function run(args: readonly string[]): CliResult {
  const [command] = args;
  if (command === undefined || command === "--version" || command === "version") {
    return { stdout: `tansu ${version}\n`, stderr: "", exitCode: 0 };
  }
  return { stdout: "", stderr: `tansu: 未対応のサブコマンド "${command}"\n`, exitCode: 2 };
}

if (import.meta.main) {
  const result = run(Bun.argv.slice(2));
  process.stdout.write(result.stdout);
  process.stderr.write(result.stderr);
  process.exit(result.exitCode);
}
