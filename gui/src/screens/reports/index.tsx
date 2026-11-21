import { useEffect, useMemo, useState } from "react";
import type { PlanResponse, ReportResponse, TaskDetailResponse, TasksResponse } from "../../api/types";
import { useApi } from "../../app/connection";
import { Badge } from "../../ui/Badge";
import { Button, type Tone } from "../../ui/Button";
import { Card, CardHead } from "../../ui/Card";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Icon, type IconName } from "../../ui/Icon";
import { ErrorPanel } from "../../ui/ResourceView";
import { Segmented } from "../../ui/Segmented";
import { StatusBadge } from "../../ui/StatusBadge";
import { Table } from "../../ui/Table";
import { useResource } from "../../ui/useResource";
import "./reports.css";

const NOT_AVAILABLE = "Not available yet";

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

/** Where the run stands, in the words the prototype uses. */
export function runStatus(r: ReportResponse): { text: string; tone: Tone } {
  if (r.broken_branch) return { text: "BROKEN", tone: "coral" };
  if (r.finished === null) return { text: "RUNNING", tone: "cyan" };
  if (r.total > 0 && r.merged === r.total) return { text: "ALL_DONE", tone: "emerald" };
  if (r.failed > 0) return { text: "FAILED", tone: "coral" };
  return { text: "PARTIAL", tone: "amber" };
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

function Kpi({ icon, tone, title, value, children }: { icon: IconName; tone: Tone; title: string; value: string; children: React.ReactNode }) {
  return (
    <Card as="section" className={`kpi tone-${tone}`} aria-label={title}>
      <div className="kpi__head">
        <Icon name={icon} size={22} />
        <h2 className="kpi__title">{title}</h2>
      </div>
      <strong className="kpi__value">{value}</strong>
      <div className="kpi__sub">{children}</div>
    </Card>
  );
}

const TIER_TONE: Record<string, Tone> = { small: "neutral", medium: "amber", high: "coral" };
function Tier({ tier }: { tier: string | null }) {
  return tier ? <Badge tone={TIER_TONE[tier] ?? "neutral"}>{tier}</Badge> : <span className="muted">—</span>;
}

type RowFilter = "all" | "rework" | "escalated";

/** What the reviewer suggested and where the coders went outside their files: from `tasks/{id}`. */
function useDetails(ids: string[]) {
  const key = ids.join(",");
  return useResource(
    async (api, signal) => {
      const settled = await Promise.allSettled(ids.map((id) => api.task(id, { signal })));
      const ok: TaskDetailResponse[] = [];
      const failed: string[] = [];
      settled.forEach((s, i) => (s.status === "fulfilled" ? ok.push(s.value) : failed.push(ids[i] ?? "?")));
      return { ok, failed };
    },
    [key],
    { refreshOn: ["task-state"] },
  );
}

function ScopeCard({ details, scope }: { details: ReturnType<typeof useDetails>; scope: string | null }) {
  const rows = (details.data?.ok ?? []).flatMap((d) => d.files_outside_likely.map((f) => ({ id: d.task.id, file: f })));
  return (
    <Card as="section" className="rep-card" aria-labelledby="rep-scope">
      <CardHead title={<span id="rep-scope">Scope Warnings (Files Outside Likely Files)</span>} icon={<Icon name="warning" />}>
        <Badge pill>{plural(rows.length, "item")}</Badge>
      </CardHead>
      <p className="rep-note">Files a milestone changed that were not in its likely files.</p>
      {details.state === "loading" ? (
        <p className="rep-note muted" role="status">
          Reading the milestones…
        </p>
      ) : rows.length === 0 ? (
        <p className="rep-empty">No milestone changed files outside its likely files.</p>
      ) : (
        <Table caption="Files changed outside the planned scope">
          <thead>
            <tr>
              <th scope="col">#</th>
              <th scope="col">Milestone</th>
              <th scope="col">File (modified)</th>
              <th scope="col">Scope</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => (
              <tr key={`${r.id}/${r.file}`}>
                <td className="mono muted">{String(i + 1).padStart(2, "0")}</td>
                <td>
                  <Badge mono>{r.id}</Badge>
                </td>
                <td>
                  <span className="path-chip">{r.file}</span>
                </td>
                <td>{scope ? <Badge tone={scope === "block" ? "coral" : scope === "off" ? "neutral" : "amber"} mono>{scope}</Badge> : <span className="muted">—</span>}</td>
              </tr>
            ))}
          </tbody>
        </Table>
      )}
      {details.data && details.data.failed.length > 0 && (
        <p className="rep-note error-text">The details of {details.data.failed.join(", ")} could not be read, so they are not counted here.</p>
      )}
    </Card>
  );
}

interface Suggestion {
  task: string;
  file: string;
  line: string | null;
  problem: string;
}

function SuggestionsCard({ details }: { details: ReturnType<typeof useDetails> }) {
  const api = useApi();
  const [asking, setAsking] = useState<Suggestion | null>(null);
  const [outcome, setOutcome] = useState<{ ok: boolean; text: string } | null>(null);
  const [sent, setSent] = useState<Set<string>>(() => new Set());
  const list: Suggestion[] = [];
  const seen = new Set<string>();
  for (const d of details.data?.ok ?? [])
    for (const r of d.reviews)
      for (const s of r.suggestions) {
        const k = `${d.task.id}\u0000${s.file}\u0000${s.problem}`;
        if (!seen.has(k)) {
          seen.add(k);
          list.push({ task: d.task.id, file: s.file, line: s.line, problem: s.problem });
        }
      }
  const keyOf = (s: Suggestion) => `${s.task}\u0000${s.file}\u0000${s.problem}`;
  const create = async (s: Suggestion) => {
    setOutcome(null);
    try {
      const res = await api.fix(s.task, { text: `${s.problem} (${s.file}${s.line ? `:${s.line}` : ""})` });
      setSent((p) => new Set(p).add(keyOf(s)));
      setOutcome({ ok: true, text: res.message || `A follow-up fix for ${s.task} was created.` });
    } catch (e) {
      setOutcome({ ok: false, text: `The follow-up fix was not created: ${e instanceof Error ? e.message : String(e)}` });
    }
  };
  return (
    <Card as="section" className="rep-card" aria-labelledby="rep-sugg">
      <CardHead title={<span id="rep-sugg">Unapplied Reviewer Suggestions</span>} icon={<Icon name="chat" />}>
        <Badge pill>{plural(list.length, "suggestion")}</Badge>
      </CardHead>
      <p className="rep-note">Non-blocking suggestions from the reviewer. A follow-up fix sends one back to the coder.</p>
      {outcome && (
        <p className={outcome.ok ? "rep-note text-emerald" : "rep-note error-text"} role={outcome.ok ? "status" : "alert"}>
          {outcome.text}
        </p>
      )}
      {details.state === "loading" ? null : list.length === 0 ? (
        <p className="rep-empty">The reviewer left no suggestions.</p>
      ) : (
        <ul className="sugg">
          {list.map((s, i) => (
            <li key={keyOf(s)}>
              <span className="dot dot--violet" aria-hidden="true" />
              <Badge mono>S{i + 1}</Badge>
              <span className="sugg__text">
                {s.problem}
                <span className="muted mono sugg__where">
                  {" "}
                  {s.task} · {s.file}
                  {s.line ? `:${s.line}` : ""}
                </span>
              </span>
              <Button size="sm" disabled={sent.has(keyOf(s))} onClick={() => setAsking(s)} aria-label={`Create a follow-up fix for suggestion S${i + 1}`}>
                <Icon name="branch" size={14} />
                {sent.has(keyOf(s)) ? "Fix created" : "Create Follow-Up Fix (orch fix)"}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {asking && (
        <ConfirmDialog
          open
          title={`Create a follow-up fix for ${asking.task}?`}
          confirmLabel="Create fix"
          onCancel={() => setAsking(null)}
          onConfirm={() => {
            const s = asking;
            setAsking(null);
            void create(s);
          }}
        >
          A small follow-up milestone is sent to the coder who built {asking.task}, with this text: “{asking.problem}”. Its check and
          review still apply.
        </ConfirmDialog>
      )}
    </Card>
  );
}

function exportJson(bundle: ReportBundle) {
  const blob = new Blob([JSON.stringify(bundle.report, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `report-${bundle.report.branch.replace(/[^\w.-]+/g, "_")}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

/** Screen 07: the run's numbers, the per-milestone and per-model tables, scope warnings and suggestions. */
export function ReportsScreen() {
  const resource = useResource<ReportBundle>(
    (api, signal) =>
      Promise.all([api.report({ signal }), api.plan({ signal }), api.tasks({ signal })]).then(
        ([report, plan, tasks]) => ({ report, plan, tasks }),
      ),
    [],
    { isEmpty: (d) => d.report.tasks.length === 0, refreshOn: ["task-state", "run-state"] },
  );
  const bundle = resource.data;
  const noRun = resource.state === "error" && resource.error?.isNoRun;

  return (
    <section className="screen rep" aria-labelledby="screen-title">
      <header className="rep__head">
        <span className="rep__icon">
          <Icon name="chart" />
        </span>
        <div className="rep__titles">
          <h1 id="screen-title" className="rep__title">
            Reports
          </h1>
          {bundle && resource.state === "ready" && <RunLine bundle={bundle} />}
        </div>
        <div className="rep__actions">
          <Button disabled={!bundle} onClick={() => bundle && exportJson(bundle)}>
            <Icon name="external" size={14} />
            Export JSON
          </Button>
          <Button unavailable={`${NOT_AVAILABLE}: summary.md is not served by the API`}>View summary.md</Button>
          <Button tone="emerald" solid unavailable={`${NOT_AVAILABLE}: run \`orch finish\``}>
            <Icon name="branch" size={14} />
            Finish &amp; Create PR
          </Button>
        </div>
      </header>

      {resource.state === "loading" && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {noRun && (
        <Card className="rep-none" role="status">
          <Icon name="chart" size={30} />
          <p className="panel-title">No report yet</p>
          <p>This project has no run yet</p>
          <p className="muted">
            The report fills in while a run goes. Write <code>.orchestrate/plan.md</code> (or ask the lead to) and run{" "}
            <code>orch sync</code>.
          </p>
        </Card>
      )}
      {resource.state === "error" && !noRun && resource.error && <ErrorPanel error={resource.error} onRetry={resource.reload} />}
      {resource.state === "empty" && (
        <Card className="rep-none" role="status">
          <p className="panel-title">No milestones</p>
          <p className="muted">This run has no milestones. Write them in .orchestrate/plan.md and run `orch sync`.</p>
        </Card>
      )}
      {resource.state === "ready" && bundle && <Report bundle={bundle} />}
    </section>
  );
}

function RunLine({ bundle: { report, plan } }: { bundle: ReportBundle }) {
  const st = runStatus(report);
  return (
    <>
      <p className="rep__run">
        Run Report: {plan.title ?? report.branch} <Badge tone={st.tone} mono>{st.text}</Badge>
      </p>
      <p className="rep__meta">
        Branch: {report.branch}
        {report.started !== null && <> · Started: {fmtTime(report.started)}</>}
        {report.finished !== null && <> · Completed: {fmtTime(report.finished)}</>}
      </p>
    </>
  );
}

function Report({ bundle }: { bundle: ReportBundle }) {
  const { report, plan, tasks } = bundle;
  const [filter, setFilter] = useState<RowFilter>("all");
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  const ids = useMemo(() => report.tasks.map((t) => t.id), [report.tasks]);
  const details = useDetails(ids);
  const toggle = (id: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const titleOf = (id: string) => plan.milestones.find((m) => m.id === id)?.title ?? "";
  const info = (id: string) => tasks.tasks.find((t) => t.id === id);
  const escalated = report.tasks.filter((r) => info(r.id)?.escalated);
  const escalatedByModel = new Map<string, number>();
  for (const t of tasks.tasks) if (t.escalated && t.model) escalatedByModel.set(t.model, (escalatedByModel.get(t.model) ?? 0) + 1);
  const plannedMinutes = plan.milestones.reduce((sum, m) => sum + (estimateMinutes(m.estimate) ?? 0), 0);
  const inProgress = report.finished === null;
  const rows = report.tasks.filter((r) =>
    filter === "rework" ? r.fix_rounds > 0 || r.review_rounds > 1 || r.conflict_rounds > 0 || r.tries > 1 : filter === "escalated" ? info(r.id)?.escalated : true,
  );

  return (
    <>
      {inProgress && (
        <p className="run-note" role="status">
          Run in progress — the numbers below are partial.
        </p>
      )}
      {report.broken_branch && (
        <p className="run-note run-note--bad" role="alert">
          The run branch fails its check: {report.broken_branch.reason}
        </p>
      )}

      <div className="kpis" aria-label="Run metrics">
        <Kpi icon="check" tone="emerald" title="First-Pass Rate" value={report.first_pass_rate === null ? "—" : `${Math.round(report.first_pass_rate * 100)}%`}>
          {report.first_pass} / {report.total} milestones passed check &amp; review on round 1
        </Kpi>
        <Kpi icon="activity" tone="cyan" title="Total Execution Time" value={fmtSeconds(report.elapsed_s)}>
          {inProgress ? (
            <span>Elapsed so far</span>
          ) : plannedMinutes > 0 && report.elapsed_s !== null ? (
            <span>
              {fmtSeconds(report.elapsed_s)} of about {fmtSeconds(plannedMinutes * 60)} planned
            </span>
          ) : (
            <span>The plan has no estimates to compare with</span>
          )}
          <span className="kpi__line">
            {report.merged} merged of {report.total}
            {report.failed > 0 && `, ${report.failed} failed`}
            {report.conflicts > 0 && `, ${plural(report.conflicts, "conflict")}`}
          </span>
        </Kpi>
        <Kpi icon="bolt" tone="violet" title="Escalations Used" value={String(escalated.length)}>
          {escalated.length === 0
            ? "No milestone needed a stronger model"
            : escalated.map((r) => (
                <span key={r.id} className="kpi__line">
                  {r.id} escalated {r.planned_tier ?? "?"} → {r.actual_tier ?? "?"}
                </span>
              ))}
        </Kpi>
        <Kpi icon="warning" tone="coral" title="Wasted Work" value={plural(report.wasted.lost_attempts, "lost attempt")}>
          {plural(report.wasted.fix_rounds, "fix round")}, {plural(report.wasted.conflict_rounds, "conflict round")},{" "}
          {plural(report.wasted.extra_attempts, "extra attempt")}
        </Kpi>
      </div>

      <div className="pair">
        <Card as="section" className="rep-card" aria-labelledby="rep-tasks">
          <CardHead title={<span id="rep-tasks">Planned Tier vs. Actual Model (Milestones)</span>} icon={<Icon name="grid" />}>
            <Segmented
              label="Filter milestones"
              value={filter}
              onChange={setFilter}
              options={[
                { value: "all", label: "All" },
                { value: "rework", label: "Rework" },
                { value: "escalated", label: "Escalated" },
              ]}
            />
            <Badge pill>{plural(rows.length, "milestone")}</Badge>
          </CardHead>
          <Table caption="Each milestone's result on this run" className="rep-tasks">
            <thead>
              <tr>
                <th scope="col">Milestone</th>
                <th scope="col">Planned Tier</th>
                <th scope="col">Worker</th>
                <th scope="col">Actual Model</th>
                <th scope="col">Rounds (Fix/Rev)</th>
                <th scope="col">Time</th>
                <th scope="col">Status</th>
                <th scope="col">
                  <span className="visually-hidden">Verification, review and files</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr>
                  <td colSpan={8} className="rep-empty">
                    No milestone matches this filter.
                  </td>
                </tr>
              ) : (
                rows.map((row) => (
                  <MilestoneRow
                    key={row.id}
                    row={row}
                    title={titleOf(row.id)}
                    worker={info(row.id)?.last_worker ?? info(row.id)?.worker ?? null}
                    open={open.has(row.id)}
                    onToggle={() => toggle(row.id)}
                  />
                ))
              )}
            </tbody>
          </Table>
        </Card>

        <Card as="section" className="rep-card" aria-labelledby="rep-models">
          <CardHead title={<span id="rep-models">Per-Model Performance Breakdown</span>} icon={<Icon name="chart" />} />
          {report.models.length === 0 ? (
            <p className="rep-empty">No model has worked on this run yet.</p>
          ) : (
            <Table caption="What each model did on this run" className="rep-models">
              <thead>
                <tr>
                  <th scope="col">Model</th>
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
                {report.models.map((m) => {
                  const pct = m.tasks ? Math.round((m.merged / m.tasks) * 100) : 0;
                  return (
                    <tr key={m.model}>
                      <td>
                        <span className="mchip">{m.model}</span> <Tier tier={m.tier} />
                      </td>
                      <td>
                        <div className="mbar">
                          <span className="mono">
                            {pct}% ({m.merged} / {m.tasks})
                          </span>
                          <span className="mbar__track" aria-hidden="true">
                            <i style={{ width: `${pct}%` }} />
                          </span>
                        </div>
                      </td>
                      <td>{m.tries}</td>
                      <td>{m.fix_rounds}</td>
                      <td>{m.review_rounds}</td>
                      <td>{m.conflict_rounds}</td>
                      <td>{m.lost}</td>
                      <td>{escalatedByModel.get(m.model) ?? 0}</td>
                    </tr>
                  );
                })}
              </tbody>
            </Table>
          )}
        </Card>
      </div>

      <div className="pair pair--b">
        <ScopeCard details={details} scope={plan.header.scope} />
        <SuggestionsCard details={details} />
      </div>
    </>
  );
}

function MilestoneRow({
  row,
  title,
  worker,
  open,
  onToggle,
}: {
  row: ReportResponse["tasks"][number];
  title: string;
  worker: string | null;
  open: boolean;
  onToggle: () => void;
}) {
  const tierText =
    row.planned_tier && row.actual_tier
      ? `${row.planned_tier} → ${row.actual_tier}${row.below_planned_tier ? " (below plan)" : ""}`
      : (row.actual_tier ?? row.planned_tier ?? "—");
  const changed = row.planned_tier && row.actual_tier && row.planned_tier !== row.actual_tier;
  return (
    <>
      <tr className="rep-row" data-milestone={row.id}>
        <th scope="row">
          <span className="rep-ms">
            <Badge mono>{row.id}</Badge> <span>{title}</span>
          </span>
        </th>
        <td>
          <span className="rep-tier" aria-hidden="true">
            <Tier tier={row.planned_tier} />
            {changed && (
              <>
                {" → "}
                <Tier tier={row.actual_tier} />
              </>
            )}
            {row.below_planned_tier && <span className="muted"> (below plan)</span>}
          </span>
          <span className="visually-hidden">{tierText}</span>
        </td>
        <td className="mono text-violet">{worker ?? <span className="muted">—</span>}</td>
        <td>{row.model ? <span className="mchip">{row.model}</span> : <span className="muted">—</span>}</td>
        <td className="mono">
          {row.fix_rounds} / {row.review_rounds}
          {row.conflict_rounds > 0 && <span className="muted"> · {plural(row.conflict_rounds, "conflict")}</span>}
        </td>
        <td>{fmtSeconds(row.seconds)}</td>
        <td>
          <StatusBadge state={row.state} />
        </td>
        <td>
          <Button size="sm" ghost aria-expanded={open} onClick={onToggle}>
            {open ? "Hide details" : "Details"}
          </Button>
        </td>
      </tr>
      {open && (
        <tr className="rep-detail-row">
          <td colSpan={8}>
            <TaskDetailPanel id={row.id} />
          </td>
        </tr>
      )}
    </>
  );
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

