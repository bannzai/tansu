import { useEffect, useRef, useState } from "react";
import { type DatabaseRows, fetchQaSession, fetchRows, type QaSession, recordQa } from "./api.ts";
import { copyText } from "./clipboard.ts";
import { Link } from "./Link.tsx";
import {
  enteredResult,
  type QaResult,
  qaProgress,
  RESULT_LABELS,
  remainingMarkdown,
  shortCommit,
  textValue,
} from "./qa.ts";
import { errorText } from "./relations.ts";
import { navigate, routePath } from "./route.ts";

/** 結果のボタンの並びと名前。 */
const RESULT_BUTTONS: readonly QaResult[] = ["ok", "ng", "later"];

/** 検証環境の入力を、次に開いた時も残すための localStorage のキー。 */
const ENV_STORAGE_KEY = "tansu.qa.env";

/** 保存した検証環境。読めない (private ブラウズ等) 時は空。 */
function storedEnv(): string {
  try {
    return window.localStorage.getItem(ENV_STORAGE_KEY) ?? "";
  } catch {
    return "";
  }
}

/** 検証環境を保存する。書けない時は保存しない (入力そのものは画面に残る)。 */
function storeEnv(env: string): void {
  try {
    window.localStorage.setItem(ENV_STORAGE_KEY, env);
  } catch {
    // 保存できなくても記録には使えるので、何もしない
  }
}

/**
 * このページを開いている間の入力を見分ける ID。run の `run_id` (`web:<この ID>:<item の行 ID>`) に入れ、同じ項目を
 * 選び直した時は run を足さずに置き換える。`crypto.randomUUID` は secure context でしか使えない (LAN の IP を http で
 * 開いた iPhone では無い) ため、secure context でなくても使える `crypto.getRandomValues` の 128 bit (UUID と同じ長さ) から作る。
 */
function newInputSessionId(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** QA の入力ビュー。project と feature で絞った item を 1 項目ずつ表示し、OK / NG / あとで + メモを run に記録する。 */
export function QaInput({ repo, feature }: { repo: string | null; feature: string | null }) {
  const [tables, setTables] = useState<{ projects: DatabaseRows; features: DatabaseRows } | null>(null);
  const [session, setSession] = useState<QaSession | null>(null);
  const [index, setIndex] = useState(0);
  const [picked, setPicked] = useState<QaResult | null>(null);
  const [note, setNote] = useState("");
  const [env, setEnv] = useState(storedEnv);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [inputSessionId] = useState(newInputSessionId);
  // 記録の応答を待つ間に履歴の移動などで project / feature が変わった時に、古い feature の応答を表示しないよう、
  // 今選んでいる project / feature を応答と照合するために持つ
  const selection = useRef({ repo, feature });
  selection.current = { repo, feature };
  // 記録の応答を待つ間は、項目の移動・結果とメモの入力・project と feature の切り替えを無効にする。応答の後に次の項目へ移り、
  // その項目の結果とメモで入力欄を置き換えるため、待つ間の操作を受け付けると消えてしまう
  const [recording, setRecording] = useState(false);

  useEffect(() => {
    Promise.all([fetchRows("project"), fetchRows("feature")]).then(
      ([projects, features]) => setTables({ projects, features }),
      (caught: unknown) => setError(`${errorText(caught)} (QA の表は tansu qa import で QA.md を取り込むと作られます)`),
    );
  }, []);

  useEffect(() => {
    setSession(null);
    if (repo === null || feature === null) {
      return;
    }
    let cancelled = false;
    fetchQaSession(repo, feature).then(
      (loaded) => {
        if (cancelled) {
          return;
        }
        setSession(loaded);
        // 開いた時は最初の未入力の項目から始める
        setIndex(
          Math.max(
            0,
            loaded.items.findIndex(({ latest_run }) => enteredResult(loaded, latest_run) === null),
          ),
        );
      },
      (caught: unknown) => {
        if (!cancelled) {
          setError(errorText(caught));
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [repo, feature]);

  const current = session?.items[index];
  // 項目を移った時と記録し直した時は、その項目の今の HEAD での結果とメモを入力欄に出す
  useEffect(() => {
    if (session === null || current === undefined) {
      return;
    }
    const entered = enteredResult(session, current.latest_run);
    setPicked(entered === "ok" || entered === "ng" || entered === "later" ? entered : null);
    setNote(entered === null ? "" : (textValue(current.latest_run, "note") ?? ""));
  }, [session, current]);

  const projectId = tables?.projects.rows.find((row) => textValue(row, "repo") === repo)?.id;
  const featureNames =
    tables?.features.rows.flatMap((row) => {
      const project = row.values.project;
      const name = textValue(row, "name");
      return project?.type === "relation" && projectId !== undefined && project.value.includes(projectId) && name
        ? [name]
        : [];
    }) ?? [];

  const record = async () => {
    if (session === null || current === undefined || picked === null || session.head_commit === null) {
      return;
    }
    setError(null);
    setRecording(true);
    try {
      await recordQa({
        item_id: current.item.id,
        result: picked,
        commit: session.head_commit,
        note: note.trim() === "" ? null : note,
        env: env.trim() === "" ? null : env.trim(),
        run_id: `web:${inputSessionId}:${current.item.id}`,
      });
      const reloaded = await fetchQaSession(session.repo, session.feature);
      setStatus(`${index + 1}. を ${RESULT_LABELS[picked]} で記録しました`);
      if (selection.current.repo === reloaded.repo && selection.current.feature === reloaded.feature) {
        setSession(reloaded);
        setIndex(Math.min(index + 1, reloaded.items.length - 1));
      }
    } catch (caught) {
      setError(errorText(caught));
    } finally {
      setRecording(false);
    }
  };

  const copyRemaining = async () => {
    if (session === null) {
      return;
    }
    try {
      await copyText(remainingMarkdown(session));
      setStatus("未入力と NG の項目をコピーしました");
    } catch (caught) {
      setError(errorText(caught));
    }
  };

  const progress = session === null ? null : qaProgress(session);
  return (
    <div className="qa">
      <header className="bar">
        <Link to={routePath({ page: "table", database: "item" })}>← item</Link>
        <h1 className="qa-title">QA の入力</h1>
        <select
          aria-label="project"
          value={repo ?? ""}
          disabled={recording}
          onChange={(event) => navigate(routePath({ page: "qa", repo: event.target.value || null, feature: null }))}
        >
          <option value="">project を選ぶ</option>
          {tables?.projects.rows.map((row) => {
            const projectRepo = textValue(row, "repo");
            return projectRepo === null ? null : (
              <option key={row.id} value={projectRepo}>
                {projectRepo}
              </option>
            );
          })}
        </select>
        <select
          aria-label="feature"
          value={feature ?? ""}
          disabled={repo === null || recording}
          onChange={(event) => navigate(routePath({ page: "qa", repo, feature: event.target.value || null }))}
        >
          <option value="">feature を選ぶ</option>
          {featureNames.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        <div className="spacer" />
        <label className="env">
          検証環境
          <input
            value={env}
            placeholder="iPhone 17 Pro / iOS 27.1"
            onChange={(event) => {
              setEnv(event.target.value);
              storeEnv(event.target.value);
            }}
          />
        </label>
        {session !== null && (
          <span className="muted" data-testid="head-commit">
            {session.head_commit === null ? "commit 不明" : `commit ${shortCommit(session.head_commit)}`}
          </span>
        )}
      </header>
      {error !== null && (
        <p role="alert" className="error qa-error">
          {error}
        </p>
      )}
      {session === null || progress === null ? (
        <p className="muted qa-empty">
          {repo === null || feature === null ? "project と feature を選んでください" : "読み込み中"}
        </p>
      ) : current === undefined ? (
        <p className="muted qa-empty">この feature に項目はありません</p>
      ) : (
        <>
          <main className="qa-main">
            <section className="qa-item" data-testid="qa-current" data-item-id={current.item.id}>
              <div className="qa-item-heading">
                <span className="qa-number">{index + 1}</span>
                <h2>{textValue(current.item, "name") ?? current.item.id}</h2>
              </div>
              <div className="tags">
                {(current.item.values.labels?.type === "multi_select" ? current.item.values.labels.value : []).map(
                  (label) => (
                    <span key={label} className="tag tag-accent">
                      {label}
                    </span>
                  ),
                )}
                {textValue(current.item, "automation") !== null && (
                  <span className="tag">
                    {textValue(current.item, "automation")}
                    {textValue(current.item, "automation_detail") === null
                      ? ""
                      : `: ${textValue(current.item, "automation_detail")}`}
                  </span>
                )}
              </div>
              <div className="box">
                <div className="section-title">手で見ること</div>
                <div>{textValue(current.item, "expected") ?? "(期待動作は書かれていません)"}</div>
              </div>
              <div className="muted small" data-testid="latest-run">
                直近の run:{" "}
                {current.latest_run === null
                  ? "なし"
                  : [
                      RESULT_LABELS[textValue(current.latest_run, "result") ?? ""] ?? "—",
                      textValue(current.latest_run, "recorded_at")?.slice(0, 10),
                      textValue(current.latest_run, "commit")?.slice(0, 7),
                    ]
                      .filter(Boolean)
                      .join(" · ")}
              </div>
              <div className="section-title">結果</div>
              <div className="result-buttons">
                {RESULT_BUTTONS.map((result) => (
                  <button
                    key={result}
                    type="button"
                    className={`result-button result-button-${result}`}
                    aria-pressed={picked === result}
                    disabled={recording}
                    onClick={() => setPicked(result)}
                  >
                    {RESULT_LABELS[result]}
                  </button>
                ))}
              </div>
              <label className="memo">
                <span className="section-title">メモ</span>
                <textarea
                  rows={4}
                  placeholder="気づいたことや NG の再現手順"
                  value={note}
                  disabled={recording}
                  onChange={(event) => setNote(event.target.value)}
                />
              </label>
              <div className="qa-nav">
                <button type="button" disabled={index === 0 || recording} onClick={() => setIndex(index - 1)}>
                  ← 前
                </button>
                <button
                  type="button"
                  className="primary"
                  disabled={picked === null || session.head_commit === null || recording}
                  onClick={record}
                >
                  記録して次へ →
                </button>
              </div>
            </section>
            <aside className="qa-list">
              <div className="section-title">この feature の項目</div>
              {session.items.map(({ item, latest_run }, itemIndex) => {
                const entered = enteredResult(session, latest_run);
                return (
                  <button
                    key={item.id}
                    type="button"
                    className={itemIndex === index ? "qa-list-item qa-list-item-current" : "qa-list-item"}
                    aria-current={itemIndex === index ? "step" : undefined}
                    disabled={recording}
                    onClick={() => setIndex(itemIndex)}
                  >
                    <span className="qa-list-number">{itemIndex + 1}</span>
                    <span className="qa-list-name">{textValue(item, "name") ?? item.id}</span>
                    <span className={entered === null ? "qa-list-result" : `qa-list-result result-text-${entered}`}>
                      {entered === null ? "" : RESULT_LABELS[entered]}
                    </span>
                  </button>
                );
              })}
            </aside>
          </main>
          <footer className="qa-footer" data-testid="qa-progress">
            <strong data-testid="qa-entered">
              {progress.entered} / {progress.total} 件 入力済み
            </strong>
            <span className="result-text-ok" data-testid="qa-ok">
              OK {progress.ok}
            </span>
            <span className="result-text-ng" data-testid="qa-ng">
              NG {progress.ng}
            </span>
            <span className="result-text-later" data-testid="qa-later">
              あとで {progress.later}
            </span>
            {status !== null && (
              <span role="status" className="muted">
                {status}
              </span>
            )}
            <div className="spacer" />
            <button type="button" className="outline" onClick={copyRemaining}>
              全部コピー
            </button>
          </footer>
        </>
      )}
    </div>
  );
}
