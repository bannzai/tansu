import type { QaSession, Row } from "./api.ts";

/** QA の入力ビューで選べる結果 (run の `result` の選択肢のうち、スキップを除いたもの)。 */
export type QaResult = "ok" | "ng" | "later";

/** run の `result` の選択肢の、画面に出す名前。 */
export const RESULT_LABELS: Readonly<Record<string, string>> = {
  ok: "OK",
  ng: "NG",
  later: "あとで",
  skip: "スキップ",
};

/** 短縮して見せる commit の桁数 (git の既定の短縮 SHA と同じ 7 桁)。 */
const SHORT_COMMIT_LENGTH = 7;

/** 行 `row` の text / url / date / select のプロパティ `property` の値。無ければ null。 */
export function textValue(row: Row | null, property: string): string | null {
  const value = row?.values[property];
  if (value === undefined) {
    return null;
  }
  switch (value.type) {
    case "text":
    case "url":
    case "date":
    case "select":
      return value.value;
    default:
      return null;
  }
}

/** commit の短縮 SHA。 */
export function shortCommit(commit: string): string {
  return commit.slice(0, SHORT_COMMIT_LENGTH);
}

/**
 * 項目の最後の run `latestRun` が、今の HEAD (`session.head_commit`) で入力した結果ならその `result`。そうでなければ
 * null (未入力)。HEAD を基準にするのは、入力ビューが「今の commit の QA」を回す画面で、別の commit の記録は
 * 再テストが要るため (`qa verified` と同じ考え方)。
 */
export function enteredResult(session: QaSession, latestRun: Row | null): string | null {
  return session.head_commit !== null && textValue(latestRun, "commit") === session.head_commit
    ? textValue(latestRun, "result")
    : null;
}

/** 進捗フッターの件数。 */
export type QaProgress = {
  /** 今の HEAD で入力した項目の数 (スキップを含む)。 */
  entered: number;
  /** 項目の数。 */
  total: number;
  /** 入力した結果が OK の項目の数。 */
  ok: number;
  /** 入力した結果が NG の項目の数。 */
  ng: number;
  /** 入力した結果が あとで の項目の数。 */
  later: number;
};

/** `session` の進捗フッターの件数。 */
export function qaProgress(session: QaSession): QaProgress {
  const results = session.items.map(({ latest_run }) => enteredResult(session, latest_run));
  const count = (result: string) => results.filter((entered) => entered === result).length;
  return {
    entered: results.filter((entered) => entered !== null).length,
    total: results.length,
    ok: count("ok"),
    ng: count("ng"),
    later: count("later"),
  };
}

/** 項目の 1 行。番号は feature の中の順 (1 始まり)。 */
function itemLine(number: number, item: Row): string {
  const expected = textValue(item, "expected");
  return `- [ ] ${number}. **${textValue(item, "name") ?? item.id}**${expected ? `: ${expected}` : ""}`;
}

/**
 * 「全部コピー」の markdown。未入力の項目と、今の HEAD で NG を入力した項目 (メモ付き) を、agent に貼って
 * 続きを頼めるよう 1 つの文書にする。どちらも無ければその旨の 1 行。
 */
export function remainingMarkdown(session: QaSession): string {
  const heading = `## QA の残り: ${session.repo} / ${session.feature} (commit ${
    session.head_commit === null ? "不明" : shortCommit(session.head_commit)
  })`;
  const numbered = session.items.map((entry, index) => ({ ...entry, number: index + 1 }));
  const unentered = numbered.filter(({ latest_run }) => enteredResult(session, latest_run) === null);
  const failed = numbered.filter(({ latest_run }) => enteredResult(session, latest_run) === "ng");
  const sections = [
    ...(unentered.length === 0
      ? []
      : [
          `### 未入力 (${unentered.length} 件)`,
          unentered.map(({ number, item }) => itemLine(number, item)).join("\n"),
        ]),
    ...(failed.length === 0
      ? []
      : [
          `### NG (${failed.length} 件)`,
          failed
            .map(({ number, item, latest_run }) => {
              const note = textValue(latest_run, "note");
              return note
                ? `${itemLine(number, item)}\n  - メモ: ${note.replaceAll("\n", "\n    ")}`
                : itemLine(number, item);
            })
            .join("\n"),
        ]),
  ];
  return [heading, ...(sections.length === 0 ? ["未入力と NG の項目はありません"] : sections)].join("\n\n");
}
