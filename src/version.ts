import packageJson from "../package.json" with { type: "json" };

/** CLI が表示するバージョン。package.json の version を唯一の正とする */
export const version: string = packageJson.version;
