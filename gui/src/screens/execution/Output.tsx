import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import { useResource } from "../../ui/useResource";
import { ErrorPanel } from "../../ui/ResourceView";
import { ExecIcon } from "./icons";
import type { Agent } from "./AgentCards";

function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1B\[[0-9;]*[a-zA-Z]/g, "");
}

const TOKEN =
  /(?<agent>\b(?:coder-\d+|reviewer|rs-[\w-]+)\b \([^)]*\))|(?<tag>\[autopilot\])|(?<mark>\((?:new|updated)\))|(?<path>(?:\.\.\/)?[a-z][\w-]*(?:\/[\w.-]+)+|\/api\/[\w/-]+|\b[\w-]+\.(?:rs|tsx?|sql)\b)/g;

function renderHighlightedLog(text: string): ReactNode {
  if (text.startsWith("$")) {
    return <span className="t-cmd">{text}</span>;
  }
  const parts: ReactNode[] = [];
  let last = 0;
  TOKEN.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = TOKEN.exec(text)) !== null) {
    if (m.index > last) {
      parts.push(text.slice(last, m.index));
    }
    const g = m.groups;
    const cls = g?.agent
      ? "t-agent"
      : g?.tag
        ? "t-tag"
        : g?.mark
          ? m[0] === "(new)"
            ? "t-new"
            : "t-upd"
          : "t-path";
    parts.push(
      <span key={m.index} className={cls}>
        {m[0]}
      </span>,
    );
    last = m.index + m[0].length;
  }
  if (last < text.length) {
    parts.push(text.slice(last));
  }
  return parts;
}

export const AUTOPILOT = "autopilot";

/** The log lines of one tab: the autopilot log, or what an agent's pane last printed. */
function useOutput(tab: string) {
  return useResource<string[]>(
    async (api, signal) => {
      if (tab === AUTOPILOT) return (await api.logs({ lines: 200 }, { signal })).lines;
      const r = await api.agentOutput(tab, { lines: 200 }, { signal });
      return r.text.split("\n");
    },
    [tab],
    { refreshOn: ["log", "task-state"] },
  );
}

/**
 * Tabbed output: autopilot plus one tab per agent (arrow keys, Home and End move between tabs).
 * A live reload swaps the lines in place; it never moves focus, and the view only follows the
 * end of the log when it was already at the end.
 */
export function Output({
  agents,
  active,
  onSelect,
}: {
  agents: Agent[];
  active: string;
  onSelect: (id: string) => void;
}) {
  const tabs = [{ id: AUTOPILOT, label: "autopilot.log" }, ...agents.map((a) => ({ id: a.name, label: `${a.name}.log` }))];
  const current = tabs.some((t) => t.id === active) ? active : AUTOPILOT;
  const out = useOutput(current);
  const lines = out.data ?? [];
  const logRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  useEffect(() => {
    const el = logRef.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [lines.length, current]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const i = tabs.findIndex((t) => t.id === current);
    let n = i;
    if (e.key === "ArrowRight") n = (i + 1) % tabs.length;
    else if (e.key === "ArrowLeft") n = (i - 1 + tabs.length) % tabs.length;
    else if (e.key === "Home") n = 0;
    else if (e.key === "End") n = tabs.length - 1;
    else return;
    e.preventDefault();
    const id = tabs[n]!.id;
    onSelect(id);
    document.getElementById(`tab-${id}`)?.focus();
  };

  return (
    <section className="card output" aria-label="Agent output">
      <div className="output__bar">
        <div className="tabs" role="tablist" aria-label="Output sources" onKeyDown={onKeyDown}>
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              className="tab"
              role="tab"
              id={`tab-${t.id}`}
              aria-selected={t.id === current}
              aria-controls="log"
              tabIndex={t.id === current ? 0 : -1}
              onClick={() => onSelect(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
        <span className="note mono">Stripped ANSI · Read-only</span>
        <span className="control">
          <button type="button" className="btn btn--cyan btn--sm" disabled aria-describedby="why-pane">
            <ExecIcon name="external" size={14} />
            Open Pane in Herdr
          </button>
          <span id="why-pane" className="visually-hidden">
            Not available yet
          </span>
        </span>
      </div>
      <div
        className="log"
        id="log"
        ref={logRef}
        role="tabpanel"
        tabIndex={0}
        aria-labelledby={`tab-${current}`}
        onScroll={(e) => {
          const el = e.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        }}
      >
        {out.state === "loading" && (
          <p role="status" className="muted log__note">
            Loading…
          </p>
        )}
        {out.state === "error" && out.error && <ErrorPanel error={out.error} onRetry={out.reload} />}
        {out.state !== "loading" && out.state !== "error" && lines.length === 0 && (
          <p className="muted log__note">No output yet.</p>
        )}
        {lines.map((line, i) => {
          const text = stripAnsi(line);
          const t = text.trim();
          return (
            <div key={i} className={`ll${t.startsWith("✓") ? " ll--ok" : t.startsWith("✗") ? " ll--bad" : ""}`}>
              <span className="ll__no">{String(i + 1).padStart(3, "0")}</span>
              <span className="ll__tx">{renderHighlightedLog(text)}</span>
            </div>
          );
        })}
      </div>
    </section>
  );
}
