import type { StatusResponse, TaskInfo } from "../../api/types";
import { ExecIcon } from "./icons";

export interface Agent {
  name: string;
  kind: string | null;
  model: string | null;
  task: TaskInfo | null;
}

/** The team on this run: `/status` workers when known, else the workers the milestones name. */
export function agentsOf(status: StatusResponse | null, tasks: TaskInfo[]): Agent[] {
  const taskFor = (name: string, id: string | null) =>
    (id ? tasks.find((t) => t.id === id) : undefined) ?? tasks.find((t) => t.worker === name) ?? null;
  if (status && status.workers.length > 0) {
    return status.workers.map((w) => {
      const task = taskFor(w.name, w.task);
      return { name: w.name, kind: w.kind, model: task?.model ?? w.model, task };
    });
  }
  const names = Array.from(new Set(tasks.map((t) => t.worker).filter((w): w is string => !!w)));
  return names.map((n) => {
    const task = taskFor(n, null);
    return { name: n, kind: null, model: task?.model ?? null, task };
  });
}

const STEP_OF: Record<string, number> = {
  coding: 0,
  "needs-fix": 0,
  verifying: 1,
  "awaiting-review": 2,
  reviewing: 2,
  reviewed: 3,
  committed: 3,
  merged: 3,
};

const BUSY = new Set(["coding", "verifying", "reviewing", "needs-fix"]);

function toneOf(state: string | null): string {
  if (!state) return "neutral";
  if (state === "failed" || state === "conflict" || state === "held") return "coral";
  if (state === "needs-answer" || state === "paused" || state === "verifying") return "amber";
  if (state === "merged" || state === "committed" || state === "reviewed") return "emerald";
  return "cyan";
}

/** code → test → review, from the milestone's state. */
function Steps({ state }: { state: string | null }) {
  const at = state ? STEP_OF[state] : undefined;
  const names = ["code", "test", "review"];
  return (
    <ol className="steps" aria-label="Steps">
      {names.map((n, i) => {
        const done = at !== undefined && i < at;
        const activeStep = at === i;
        return (
          <li key={n} className={`step${done ? " step--done" : activeStep ? " step--active" : ""}`}>
            <i>{done ? <ExecIcon name="check" size={12} /> : null}</i>
            {n}
            <span className="visually-hidden">{done ? " (done)" : activeStep ? " (now)" : ""}</span>
          </li>
        );
      })}
    </ol>
  );
}

/** One card per agent on the run. Its head opens that agent's output tab. */
export function AgentCards({ agents, onShowOutput }: { agents: Agent[]; onShowOutput: (name: string) => void }) {
  if (agents.length === 0) {
    return (
      <section className="agents agents--empty card" aria-label="Agents on this run">
        <p className="muted">No agent is on this run yet. Agents appear here once the event manager dispatches a milestone.</p>
      </section>
    );
  }
  return (
    <section className="agents" aria-label="Agents on this run">
      {agents.map((a) => {
        const state = a.task?.state ?? null;
        const busy = state !== null && BUSY.has(state);
        const badge = state ?? "idle";
        const tone = toneOf(state);
        return (
          <article className="card acard" aria-label={a.name} key={a.name}>
            <button
              type="button"
              className="acard__head"
              aria-label={`${a.name}, ${badge}. Show output`}
              onClick={() => onShowOutput(a.name)}
            >
              <span className={`dot dot--${busy ? "emerald dot--pulse" : tone === "coral" ? "coral" : "idle"}`} aria-hidden="true" />
              <span className="acard__icon">
                <ExecIcon name="user" size={18} />
              </span>
              <span className="acard__name">
                {a.name}
                {(a.kind || a.model) && (
                  <span className="mono">
                    {[a.kind, a.model].filter(Boolean).join(": ")}
                  </span>
                )}
              </span>
              <span className={`badge badge--mono badge--${tone}`}>{badge}</span>
            </button>

            <div className="acard__rows">
              <div className="arow">
                <ExecIcon name="doc" />
                <span className="arow__label">Task:</span>
                <span className="arow__val mono">{a.task ? `${a.task.id}: ${a.task.title}` : "Idle (waiting for work)"}</span>
              </div>
              {a.task && (
                <div className="arow">
                  <ExecIcon name="sync" />
                  <span className="arow__label">Attempt:</span>
                  <span className="arow__val">
                    {a.task.attempt}
                    <span className="muted">
                      {" "}
                      · fix {a.task.fix_rounds} · review {a.task.review_rounds}
                    </span>
                  </span>
                </div>
              )}
              <div className="arow arow--steps">
                <ExecIcon name="list" />
                <span className="arow__label">Step:</span>
                <Steps state={state} />
              </div>
              {a.task?.reason ? (
                <div className="arow">
                  <ExecIcon name={tone === "coral" ? "warning" : "doc"} />
                  <span className="arow__label">Notes:</span>
                  <span className={`arow__val${tone === "coral" ? " arow__val--bad" : ""}`}>{a.task.reason}</span>
                </div>
              ) : null}
            </div>
          </article>
        );
      })}
    </section>
  );
}
