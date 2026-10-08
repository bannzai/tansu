#!/usr/bin/env bun
// tansu の CLI の入口。サブコマンド (db / query / upsert / delete / export / qa / serve / mcp) は
// ロードマップの子 issue で足す。立ち上げの時点では version の表示だけを持ち、CI が端から端まで通ることを確かめる。
import { version } from "./version";

/** CLI の引数を受けて、標準出力に書く文字列と exit code を返す。I/O を伴わないためテストから直接呼べる */
export function run(args: readonly string[]): { stdout: string; exitCode: number } {
  const [command] = args;
  if (command === undefined || command === "--version" || command === "version") {
    return { stdout: `tansu ${version}\n`, exitCode: 0 };
  }
  return { stdout: `tansu: 未対応のサブコマンド "${command}"\n`, exitCode: 2 };
}

if (import.meta.main) {
  const result = run(Bun.argv.slice(2));
  process.stdout.write(result.stdout);
  process.exit(result.exitCode);
}
