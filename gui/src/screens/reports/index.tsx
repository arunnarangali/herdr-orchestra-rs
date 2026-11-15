import { useEffect, useState } from "react";
import type { PlanResponse, ReportResponse, TaskDetailResponse, TasksResponse } from "../../api/types";
import { useApi } from "../../app/connection";
import { ResourceView } from "../../ui/ResourceView";
import { StatusBadge } from "../../ui/StatusBadge";
import { useResource } from "../../ui/useResource";
import "./reports.css";

interface ReportBundle {
  report: ReportResponse;
  plan: PlanResponse;
  tasks: TasksResponse;
}

/** `1700000360` as a readable time; the number comes from the run, not the clock. */
function fmtTime(epochSeconds: number | null): string {
  if (epochSeconds === null) return "";
  return new Date(epochSeconds * 1000).toLocaleString();
}

/** `3660` as `1h 1m`. */
export function fmtSeconds(secs: number | null): string {
  if (secs === null) return "—";
  const h = Math.floor(secs / 3600);
  const m = Math.round((secs % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

/** An estimate line like `75m` or `1h 30m` as minutes; null when it is not parseable. */
export function estimateMinutes(text: string | null): number | null {
  if (!text) return null;
  let total = 0;
  let matched = false;
  for (const match of text.matchAll(/(\d+)\s*(h|m|min)/g)) {
    const n = Number(match[1]);
    if (Number.isNaN(n)) continue;
    matched = true;
    total += match[2] === "h" ? n * 60 : n;
  }
  return matched ? total : null;
}

function Checks({ detail }: { detail: TaskDetailResponse }) {
  const result = detail.results[detail.results.length - 1];
  return (
    <div className="task-detail-block">
      <h4>Verification (check)</h4>
      {detail.checks.length === 0 && !result?.check && <p className="muted">no check recorded yet</p>}
      <ul>
        {result?.check && (
          <li key="result">
            last result: {result.check.ran ? (result.check.passed ? "passed" : "failed") : "not run"}
            {result.check.summary && ` — ${result.check.summary}`}
          </li>
        )}
        {detail.checks.map((c, i) => (
          <li key={i}>
            {c.ok ? "passed" : "failed"} on the {c.where} in {c.secs.toFixed(1)}s at {fmtTime(c.ts)}
          </li>
        ))}
      </ul>
    </div>
  );
}

function Findings({ kind, items }: { kind: string; items: TaskDetailResponse["reviews"][number]["blockers"] }) {
  if (items.length === 0) return null;
  return (
    <div>
      <h5>{kind}</h5>
      <ul className="finding-list">
        {items.map((f, i) => (
          <li key={i}>
            <code>
              {f.file}
              {f.line ? `:${f.line}` : ""}
            </code>{" "}
            {f.problem}
            {f.criterion && <span className="muted"> ({f.criterion})</span>}
            {f.why && <span className="muted"> — {f.why}</span>}
          </li>
        ))}
      </ul>
    </div>
  );
}

function Reviews({ detail }: { detail: TaskDetailResponse }) {
  if (detail.reviews.length === 0) return <p className="muted">no review yet</p>;
  return (
    <div className="task-detail-block">
      <h4>Review findings</h4>
      {detail.reviews.map((r) => (
        <div key={r.round} className="review-round">
          <p>
            round {r.round}
            {r.outcome && <>: {r.outcome}</>}
            {r.error && <>: unreadable ({r.error})</>}
          </p>
          {r.criteria.length > 0 && (
            <ul>
              {r.criteria.map((c) => (
                <li key={c.id}>
                  {c.id}: {c.met ? "met" : "not met"}
                  {c.note && ` — ${c.note}`}
                </li>
              ))}
            </ul>
          )}
          <Findings kind="Blockers" items={r.blockers} />
          <Findings kind="Suggestions" items={r.suggestions} />
        </div>
      ))}
    </div>
  );
}

function FilesChanged({ detail }: { detail: TaskDetailResponse }) {
  if (detail.files_changed.length === 0 && detail.files_outside_likely.length === 0)
    return <p className="muted">no files recorded</p>;
  return (
    <div className="task-detail-block">
      <h4>Files changed</h4>
      <ul className="file-list">
        {detail.files_changed.map((f) => (
          <li key={f}>
            <code>{f}</code>
          </li>
        ))}
      </ul>
      {detail.files_outside_likely.length > 0 && (
        <p className="muted">
          outside the milestone's likely files: {detail.files_outside_likely.join(", ")}
        </p>
      )}
    </div>
  );
}

/** The check a coder ran, the review findings and the files changed, from `tasks/{id}`. */
function TaskDetailPanel({ id }: { id: string }) {
  const api = useApi();
  const [state, setState] = useState<"loading" | "ready" | "error">("loading");
  const [detail, setDetail] = useState<TaskDetailResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const abort = new AbortController();
    setState("loading");
    void (async () => {
      try {
        const d = await api.task(id, { signal: abort.signal });
        if (!abort.signal.aborted) {
          setDetail(d);
          setState("ready");
        }
      } catch (e) {
        if (!abort.signal.aborted) {
          setError(e instanceof Error ? e.message : String(e));
          setState("error");
        }
      }
    })();
    return () => abort.abort();
  }, [api, id]);
  if (state === "loading") return <div role="status">Loading {id} details…</div>;
  if (state === "error") return <p className="muted">could not load {id}: {error}</p>;
  if (!detail) return null;
  return (
    <div className="task-detail">
      <Checks detail={detail} />
      <Reviews detail={detail} />
      <FilesChanged detail={detail} />
    </div>
  );
}

/** Run numbers, per-model table and per-milestone results, for the current run. */
export function ReportsScreen() {
  const resource = useResource<ReportBundle>(
    (api, signal) =>
      Promise.all([api.report({ signal }), api.plan({ signal }), api.tasks({ signal })]).then(
        ([report, plan, tasks]) => ({ report, plan, tasks }),
      ),
    [],
    { isEmpty: (d) => d.report.tasks.length === 0, refreshOn: ["task-state", "run-state"] },
  );
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  const toggle = (id: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const renderReport = ({ report, plan, tasks }: ReportBundle) => {
    const titleOf = (id: string) => plan.milestones.find((m) => m.id === id)?.title ?? "";
    const escalatedByModel = new Map<string, number>();
    for (const t of tasks.tasks)
      if (t.escalated && t.model)
        escalatedByModel.set(t.model, (escalatedByModel.get(t.model) ?? 0) + 1);
    const plannedMinutes = plan.milestones.reduce((sum, m) => sum + (estimateMinutes(m.estimate) ?? 0), 0);
    const anyEstimate = plan.milestones.some((m) => estimateMinutes(m.estimate) !== null);
    const inProgress = report.finished === null;
    return (
      <>
        {inProgress && (
          <p className="run-note" role="status">
            Run in progress — the numbers below are partial.
          </p>
        )}
        <section className="panel" aria-labelledby="rep-numbers">
          <h2 id="rep-numbers">Run report</h2>
          <dl className="rep-numbers">
            <div>
              <dt>Branch</dt>
              <dd>
                <code>{report.branch}</code>
              </dd>
            </div>
            <div>
              <dt>Started</dt>
              <dd>{fmtTime(report.started)}</dd>
            </div>
            <div>
              <dt>{inProgress ? "Elapsed so far" : "Finished"}</dt>
              <dd>
                {inProgress ? fmtSeconds(report.elapsed_s ?? null) : fmtTime(report.finished)}
                {inProgress && report.finished === null && report.started !== null && " (running)"}
              </dd>
            </div>
            {anyEstimate && plannedMinutes > 0 && report.elapsed_s !== null && (
              <div>
                <dt>Time against estimate</dt>
                <dd>
                  {fmtSeconds(report.elapsed_s)} of about {fmtSeconds(plannedMinutes * 60)} planned
                </dd>
              </div>
            )}
            <div>
              <dt>Milestones</dt>
              <dd>
                {report.merged} merged of {report.total}
                {report.failed > 0 && `, ${report.failed} failed`}
              </dd>
            </div>
            <div>
              <dt>First-pass rate</dt>
              <dd>{report.first_pass_rate === null ? "—" : `${Math.round(report.first_pass_rate * 100)}%`}</dd>
            </div>
            <div>
              <dt>Conflicts</dt>
              <dd>{report.conflicts}</dd>
            </div>
            <div>
              <dt>Wasted work</dt>
              <dd>
                {report.wasted.lost_attempts} lost attempts, {report.wasted.fix_rounds} fix rounds,{" "}
                {report.wasted.conflict_rounds} conflict rounds, {report.wasted.extra_attempts} extra
                attempts
              </dd>
            </div>
          </dl>
          {report.broken_branch && (
            <p className="error-text">The run branch fails its check: {report.broken_branch.reason}</p>
          )}
        </section>

        <section className="panel" aria-labelledby="rep-models">
          <h2 id="rep-models">Models</h2>
          <table className="rep-table">
            <caption className="visually-hidden">What each model did on this run</caption>
            <thead>
              <tr>
                <th scope="col">Model</th>
                <th scope="col">Tier</th>
                <th scope="col">Tasks</th>
                <th scope="col">Merged</th>
                <th scope="col">Attempts</th>
                <th scope="col">Fix rounds</th>
                <th scope="col">Review rounds</th>
                <th scope="col">Conflict rounds</th>
                <th scope="col">Lost</th>
                <th scope="col">Escalations</th>
              </tr>
            </thead>
            <tbody>
              {report.models.map((m) => (
                <tr key={m.model}>
                  <td>
                    <code>{m.model}</code>
                  </td>
                  <td>{m.tier ?? "—"}</td>
                  <td>{m.tasks}</td>
                  <td>{m.merged}</td>
                  <td>{m.tries}</td>
                  <td>{m.fix_rounds}</td>
                  <td>{m.review_rounds}</td>
                  <td>{m.conflict_rounds}</td>
                  <td>{m.lost}</td>
                  <td>{escalatedByModel.get(m.model) ?? 0}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>

        <section aria-labelledby="rep-tasks">
          <h2 id="rep-tasks">Milestones</h2>
          <table className="rep-table">
            <caption className="visually-hidden">Each milestone's result on this run</caption>
            <thead>
              <tr>
                <th scope="col">Milestone</th>
                <th scope="col">State</th>
                <th scope="col">Tries</th>
                <th scope="col">Fix</th>
                <th scope="col">Review</th>
                <th scope="col">Conflicts</th>
                <th scope="col">Tier</th>
                <th scope="col">Model</th>
                <th scope="col">Time</th>
                <th scope="col">
                  <span className="visually-hidden">Verification, review and files</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {report.tasks.map((row) => (
                <FragmentRow
                  key={row.id}
                  row={row}
                  title={titleOf(row.id)}
                  open={open.has(row.id)}
                  onToggle={() => toggle(row.id)}
                />
              ))}
            </tbody>
          </table>
        </section>
      </>
    );
  };

  return (
    <section className="screen" aria-labelledby="screen-title">
      <h1 id="screen-title">Reports</h1>
      <ResourceView
        resource={resource}
        empty="This run has no milestones. Write them in .orchestrate/plan.md and run `orch sync`."
      >
        {renderReport}
      </ResourceView>
    </section>
  );
}

function FragmentRow({
  row,
  title,
  open,
  onToggle,
}: {
  row: ReportResponse["tasks"][number];
  title: string;
  open: boolean;
  onToggle: () => void;
}) {
  const tier =
    row.planned_tier && row.actual_tier
      ? `${row.planned_tier} → ${row.actual_tier}${row.below_planned_tier ? " (below plan)" : ""}`
      : (row.actual_tier ?? row.planned_tier ?? "—");
  return (
    <>
      <tr className="rep-row" data-milestone={row.id}>
        <th scope="row">
          <code>{row.id}</code> {title}
        </th>
        <td>
          <StatusBadge state={row.state} />
        </td>
        <td>{row.tries}</td>
        <td>{row.fix_rounds}</td>
        <td>{row.review_rounds}</td>
        <td>{row.conflict_rounds}</td>
        <td>{tier}</td>
        <td>
          <code>{row.model ?? "—"}</code>
        </td>
        <td>{fmtSeconds(row.seconds)}</td>
        <td>
          <button
            type="button"
            className="button rep-expand"
            aria-expanded={open}
            onClick={onToggle}
          >
            {open ? "Hide details" : "Details"}
          </button>
        </td>
      </tr>
      {open && (
        <tr className="rep-detail-row">
          <td colSpan={10}>
            <TaskDetailPanel id={row.id} />
          </td>
        </tr>
      )}
    </>
  );
}
