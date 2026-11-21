import { useState, type KeyboardEvent } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useApi } from "../../app/connection";
import type { PlanMilestone, PlanResponse, TasksResponse } from "../../api/types";
import { Modal } from "../../ui/Modal";
import { ResourceView } from "../../ui/ResourceView";
import { StatusBadge } from "../../ui/StatusBadge";
import { useResource } from "../../ui/useResource";
import { useToast } from "../../ui/Toast";
import { MilestoneGraph } from "./Graph";
import { MilestoneDetail } from "./MilestoneDetail";
import { PlanApproval } from "./PlanApproval";
import { WfIcon } from "./icons";
import "./workflows.css";

interface PlanTasksStatus {
  plan: PlanResponse;
  tasks: TasksResponse;
  /** `/status` run_state, or null when the read failed (the milestones still say what is live). */
  runState: string | null;
}

const TIER_TONE: Record<string, string> = {
  small: "neutral",
  medium: "amber",
  high: "coral",
};

const STATE_CONFIG: Record<string, { label: string; tone: string }> = {
  merged: { label: "Merged", tone: "emerald" },
  coding: { label: "Coding", tone: "cyan" },
  verifying: { label: "Verifying", tone: "amber" },
  failed: { label: "Failed", tone: "coral" },
  pending: { label: "Pending", tone: "neutral" },
};

function stateBadge(state: string) {
  const st = STATE_CONFIG[state] ?? STATE_CONFIG["pending"]!;
  const cls = st.tone === "neutral" ? "badge" : `badge badge--${st.tone}`;
  return (
    <span className={cls}>
      {st.label}
      {state === "merged" ? " ✓" : ""}
    </span>
  );
}

function stateIcon(state: string, small?: boolean) {
  const iconName =
    state === "merged"
      ? "check"
      : state === "coding" || state === "verifying"
        ? "spinner"
        : state === "failed"
          ? "warning"
          : "circle";
  const label = (STATE_CONFIG[state] ?? STATE_CONFIG["pending"]!).label;
  return (
    <span
      className={`sicon sicon--${state}${small ? " sicon--sm" : ""}`}
      role="img"
      aria-label={label}
    >
      <WfIcon
        name={iconName}
        size={small ? 14 : 18}
        className={iconName === "spinner" ? "spin" : ""}
      />
    </span>
  );
}

function tierBadge(tier: string | null) {
  if (!tier) return null;
  const tone = TIER_TONE[tier] ?? "neutral";
  const cls = tone === "neutral" ? "badge" : `badge badge--${tone}`;
  return <span className={cls}>Tier: {tier}</span>;
}

export function WorkflowsScreen() {
  const api = useApi();
  const toast = useToast();
  const navigate = useNavigate();

  const resource = useResource<PlanTasksStatus>(
    (client, signal) =>
      Promise.all([
        client.plan({ signal }),
        client.tasks({ signal }),
        client.status({ signal }).then((s) => s.run_state, () => null),
      ]).then(([plan, tasks, runState]) => ({ plan, tasks, runState })),
    [],
    {
      isEmpty: (d) => d.plan.milestones.length === 0,
      refreshOn: ["task-state", "run-state"],
    },
  );

  const [view, setView] = useState<"dag" | "list">("dag");
  const [showDeps, setShowDeps] = useState(true);
  const [editOpen, setEditOpen] = useState(false);
  const [params, setParams] = useSearchParams();

  const milestones = resource.data?.plan.milestones ?? [];
  const tasks = resource.data?.tasks.tasks ?? [];

  const taskOf = (id: string) => tasks.find((t) => t.id === id);

  // Deep link: `?milestone=M2` on the screen's own route (the app routes by hash, so the
  // address must stay inside the route). An unknown id falls back to the first milestone.
  const wanted = params.get("milestone");
  const selected =
    milestones.find((m) => m.id === wanted)?.id ?? milestones[0]?.id ?? null;

  const select = (id: string) =>
    setParams(
      (p) => {
        const n = new URLSearchParams(p);
        n.set("milestone", id);
        return n;
      },
      { replace: true },
    );

  const openEdit = (_field: string = "base") => setEditOpen(true);

  // Keyboard navigation through milestone rows
  const handleMsRowsKeyDown = (e: KeyboardEvent<HTMLUListElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const currentBtn = (e.target as HTMLElement).closest<HTMLButtonElement>(".mrow__main");
    if (!currentBtn) return;
    e.preventDefault();
    const allButtons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>(".mrow__main"));
    const idx = allButtons.indexOf(currentBtn);
    if (idx === -1) return;
    const nextIdx = e.key === "ArrowDown" ? idx + 1 : idx - 1;
    const nextBtn = allButtons[nextIdx];
    if (nextBtn) nextBtn.focus();
  };

  // Keyboard navigation for view switcher
  const handleViewSegKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      setView((v) => (v === "dag" ? "list" : "dag"));
    }
  };

  // A run is live when the server says so, or when any milestone is mid-flight or waiting on the
  // lead or developer (anything started and not yet merged or ended).
  const IN_FLIGHT = new Set(["coding", "verifying", "reviewing", "awaiting-review", "needs-fix", "held", "needs-answer", "committed"]);
  const isRunLive = resource.data?.runState === "running" || tasks.some((t) => IN_FLIGHT.has(t.state));

  const handleStartRun = async () => {
    if (isRunLive) {
      toast(`Run is already live. Open the Execution Monitor to follow it.`, {
        tone: "cyan",
      });
      return;
    }

    // Only milestones that are included and not yet done: never merged or excluded work.
    const ids = milestones
      .filter((m) => {
        if (!m.enabled) return false;
        const t = taskOf(m.id);
        return !t || t.state === "pending" || t.state === "paused" || t.state === "failed";
      })
      .map((m) => m.id);
    if (ids.length === 0) {
      toast("Nothing to run: every included milestone is already merged.", { tone: "amber" });
      return;
    }

    try {
      await api.dispatch({ ids });
      toast(
        `Run started with ${ids.length} milestone${ids.length === 1 ? "" : "s"}. Opening Execution Monitor…`,
        { tone: "emerald" },
      );
      navigate("/execution");
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      toast(`Failed to start run: ${msg}`, { tone: "coral" });
    }
  };

  const renderContent = (data: PlanTasksStatus) => {
    const { plan: p, tasks: t } = data;
    const curMilestone: PlanMilestone | undefined =
      p.milestones.find((m) => m.id === selected) ?? p.milestones[0];
    const curTask = curMilestone ? taskOf(curMilestone.id) : undefined;
    const h = p.header;

    return (
      <div className="wf">
        <PlanApproval tasks={t.tasks} />

        {/* Plan Header Card */}
        <section className="card plan-head" aria-label="Plan settings" aria-labelledby="plan-title">
          <h2 className="visually-hidden">Plan settings</h2>
          <div className="plan-head__top">
            <span className="plan-head__icon" id="plan-icon">
              <WfIcon name="branch" size={20} />
            </span>
            <h1 className="plan-head__title mono" id="plan-title">
              <span className="hash">#</span>
              <span className="name">{p.title || "Plan"}</span>
            </h1>
            <div className="plan-head__actions">
              <button
                type="button"
                className="btn"
                id="sync-btn"
                disabled
                title="Not available yet"
              >
                <span className="btn__icon">
                  <WfIcon name="sync" />
                </span>
                Sync Plan
              </button>
              <button
                type="button"
                className="btn"
                id="edit-btn"
                aria-haspopup="dialog"
                onClick={() => openEdit("base")}
              >
                <span className="btn__icon">
                  <WfIcon name="pencil" />
                </span>
                Edit Plan
              </button>
              <button
                type="button"
                className="btn btn--emerald btn--solid"
                id="start-btn"
                disabled={p.milestones.length === 0}
                onClick={() => void handleStartRun()}
              >
                <span className="btn__icon">
                  <WfIcon name="play" />
                </span>
                Start Run
              </button>
            </div>
          </div>

          <ul className="plan-head__chips" id="plan-chips" aria-label="Plan settings">
            <li>
              <button
                type="button"
                className="pchip pchip--violet"
                title="Edit base"
                aria-haspopup="dialog"
                onClick={() => openEdit("base")}
              >
                <WfIcon name="branch" />
                Base: <span className="mono">{h.base || "main"}</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className="pchip pchip--cyan"
                title="Edit branch"
                aria-haspopup="dialog"
                onClick={() => openEdit("branch")}
              >
                <WfIcon name="branch" />
                Branch: <span className="mono">{h.branch || "—"}</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className="pchip pchip--cyan"
                title="Edit check"
                aria-haspopup="dialog"
                onClick={() => openEdit("check")}
              >
                <WfIcon name="terminal" />
                Check: <span className="mono">{h.check_command || "—"}</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className="pchip pchip--violet"
                title="Edit isolation"
                aria-haspopup="dialog"
                onClick={() => openEdit("isolation")}
              >
                <WfIcon name="layers" />
                Isolation: <span className="mono">{h.isolation || "worktree"}</span>
              </button>
            </li>
            {h.max_parallel && (
              <li>
                <button
                  type="button"
                  className="pchip pchip--violet"
                  title="Max parallel"
                  onClick={() => openEdit("isolation")}
                >
                  <WfIcon name="layers" />
                  Max parallel: <span className="mono">{h.max_parallel}</span>
                </button>
              </li>
            )}
            <li>
              <button
                type="button"
                className={`pchip ${h.scope === "off" ? "pchip--muted" : "pchip--amber"}`}
                title="Edit scope"
                aria-haspopup="dialog"
                onClick={() => openEdit("scope")}
              >
                <WfIcon name="warning" />
                Scope: <span className="mono">{h.scope || "warn"}</span>
              </button>
            </li>
            {h.workers && (
              <li>
                <button
                  type="button"
                  className="pchip pchip--violet"
                  title="Workers"
                  onClick={() => openEdit("base")}
                >
                  <WfIcon name="user" />
                  Workers: <span className="mono">{h.workers}</span>
                </button>
              </li>
            )}
            {h.reviewer && (
              <li>
                <button
                  type="button"
                  className="pchip pchip--violet"
                  title="Reviewer"
                >
                  <WfIcon name="user" />
                  Reviewer: <span className="mono">{h.reviewer}</span>
                </button>
              </li>
            )}
            <li>
              <button
                type="button"
                className={`pchip ${h.risk_hold && h.risk_hold !== "off" ? "pchip--coral" : "pchip--muted"}`}
                title="Edit risk hold"
                aria-haspopup="dialog"
                onClick={() => openEdit("risk")}
              >
                <WfIcon name="shieldalert" />
                Risk hold: <span className="mono">{h.risk_hold && h.risk_hold !== "off" ? "on" : "off"}</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className="pchip pchip--violet"
                title="Edit commits"
                aria-haspopup="dialog"
                onClick={() => openEdit("commits")}
              >
                <WfIcon name="code" />
                Commits: <span className="mono">{h.commits || "per-milestone"}</span>
              </button>
            </li>
          </ul>
        </section>

        {/* Feature breakdown if present */}
        {p.feature_breakdown && (
          <section className="card" aria-labelledby="wf-breakdown" style={{ padding: "12px 16px" }}>
            <h2 id="wf-breakdown" style={{ fontSize: 15, fontWeight: 600, margin: "0 0 6px" }}>
              Feature breakdown
            </h2>
            <p className="prewrap" style={{ fontSize: 13, color: "var(--text-muted)" }}>
              {p.feature_breakdown}
            </p>
          </section>
        )}

        {/* Execution Plan (DAG) Card */}
        <section className="card dag" aria-labelledby="dag-title">
          <header className="card__head">
            <h2 className="card__title" id="dag-title">
              <WfIcon name="dag" />
              Execution Plan (DAG) — Parallel &amp; Dependency View
            </h2>
            <div
              className="seg"
              role="group"
              aria-label="Plan view"
              tabIndex={0}
              onKeyDown={handleViewSegKeyDown}
            >
              <button
                type="button"
                id="view-dag"
                aria-pressed={view === "dag"}
                onClick={() => setView("dag")}
              >
                <WfIcon name="dag" />
                DAG
              </button>
              <button
                type="button"
                id="view-list"
                aria-pressed={view === "list"}
                onClick={() => setView("list")}
              >
                <WfIcon name="list" />
                List
              </button>
            </div>
          </header>

          {view === "list" ? (
            <div className="dag__body">
              <div className="dag__list">
                <table className="table" aria-label="Milestones as a table">
                  <thead>
                    <tr>
                      <th scope="col">ID</th>
                      <th scope="col">Milestone</th>
                      <th scope="col">Tier</th>
                      <th scope="col">State</th>
                      <th scope="col">Agent</th>
                      <th scope="col">Depends on</th>
                    </tr>
                  </thead>
                  <tbody>
                    {p.milestones.map((m) => {
                      const tInfo = taskOf(m.id);
                      const state = tInfo?.state ?? "pending";
                      return (
                        <tr key={m.id} className={m.enabled ? "" : "muted"}>
                          <td>
                            <button
                              type="button"
                              className="btn btn--sm mono"
                              aria-pressed={m.id === selected}
                              onClick={() => select(m.id)}
                            >
                              {m.id}
                            </button>
                          </td>
                          <td>{m.title}</td>
                          <td>{tierBadge(m.tier)}</td>
                          <td>{stateBadge(state)}</td>
                          <td className="mono">{tInfo?.worker ?? "—"}</td>
                          <td className="mono">
                            {m.depends_on.length > 0
                              ? m.depends_on.map((d) => d.id).join(", ")
                              : "—"}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </div>
          ) : (
            <MilestoneGraph
              milestones={p.milestones}
              tasks={t.tasks}
              selected={selected}
              onSelect={select}
              showDeps={showDeps}
            />
          )}

          {/* Accessible list alongside graph */}
          <ul className="graph-list visually-hidden" aria-label="Milestones as a list">
            {p.milestones.map((m) => {
              const tInfo = taskOf(m.id);
              return (
                <li key={m.id} className="graph-list-item">
                  <button type="button" className="btn btn--sm mono" onClick={() => select(m.id)}>
                    <code>{m.id}</code> {m.title}
                  </button>
                  <StatusBadge state={tInfo?.state ?? "pending"} />
                  <span className="muted graph-list-deps">
                    {m.depends_on.length > 0
                      ? `after ${m.depends_on.map((d) => (d.reason ? `${d.id} — ${d.reason}` : d.id)).join("; ")}`
                      : "no dependencies"}
                  </span>
                </li>
              );
            })}
          </ul>
        </section>

        {/* Bottom grid: Milestones list + Detail pane */}
        <div className="wf-bottom">
          <section className="card ms-card" aria-labelledby="ms-title">
            <header className="card__head">
              <h2 className="card__title" id="ms-title" aria-label="Milestones">
                <WfIcon name="layers" />
                {`Milestones (${p.milestones.length})`}
              </h2>
              <label className="check ms-card__deps">
                <input
                  type="checkbox"
                  id="show-deps"
                  checked={showDeps}
                  onChange={(e) => setShowDeps(e.target.checked)}
                />
                <span>Show dependencies</span>
              </label>
            </header>

            <ul
              className="ms-rows"
              id="ms-rows"
              role="list"
              onKeyDown={handleMsRowsKeyDown}
            >
              {p.milestones.map((m) => {
                const tInfo = taskOf(m.id);
                const state = tInfo?.state ?? "pending";
                const isSel = m.id === selected;
                return (
                  <li
                    key={m.id}
                    className={`mrow${isSel ? " mrow--sel" : ""}`}
                    data-milestone={m.id}
                  >
                    <input
                      type="checkbox"
                      checked={m.enabled}
                      disabled
                      title="Not available yet"
                      aria-label={`Include ${m.id} in the run`}
                    />
                    <button
                      type="button"
                      className="mrow__main"
                      aria-current={isSel ? "true" : undefined}
                      onClick={() => select(m.id)}
                    >
                      <span className="mrow__id">{m.id}</span>
                      <span>
                        <span className="mrow__title">{m.title}</span>
                        <span className="mrow__why">
                          {"Why separate: "}
                          {m.why_separate || m.why || "—"}
                          {showDeps && m.depends_on.length > 0 ? (
                            <span>
                              {` · after ${m.depends_on.map((d) => (d.reason ? `${d.id} — ${d.reason}` : d.id)).join("; ")}`}
                            </span>
                          ) : null}
                        </span>
                      </span>
                      {tInfo?.worker && <span className="ms-card-worker">{tInfo.worker}</span>}
                      {tInfo?.model && <code className="ms-card-model">{tInfo.model}</code>}
                      {tInfo?.state && <span className="visually-hidden">{tInfo.state}</span>}
                      {tierBadge(m.tier)}
                      <span className="mrow__state">{stateIcon(state, true)}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </section>

          <section
            className="card detail"
            id="detail"
            aria-live="polite"
            aria-labelledby="detail-title"
          >
            {curMilestone && <MilestoneDetail milestone={curMilestone} task={curTask} />}
          </section>
        </div>

        {/* Edit Plan Dialog */}
        <Modal open={editOpen} title="Edit plan settings" onClose={() => setEditOpen(false)} initialFocus="#edit-cancel">
          <form onSubmit={(e) => e.preventDefault()}>
            <div className="modal__body">
              <p className="hint muted" style={{ marginBottom: 12 }}>
                Not available yet: API does not support editing the plan today (see M6).
              </p>
              <div className="grid-2">
                <div className="field">
                  <label htmlFor="ed-base">Base branch</label>
                  <input
                    className="input mono"
                    id="ed-base"
                    name="base"
                    defaultValue={h.base || ""}
                    disabled
                  />
                </div>
                <div className="field">
                  <label htmlFor="ed-branch">Feature branch</label>
                  <input
                    className="input mono"
                    id="ed-branch"
                    name="branch"
                    defaultValue={h.branch || ""}
                    disabled
                  />
                </div>
              </div>
              <div className="field">
                <label htmlFor="ed-check">Check command</label>
                <input
                  className="input mono"
                  id="ed-check"
                  name="check"
                  defaultValue={h.check_command || ""}
                  disabled
                />
                <span className="hint">Run by autopilot before a milestone can merge.</span>
              </div>
              <div className="grid-2">
                <div className="field">
                  <label htmlFor="ed-isolation">Isolation</label>
                  <select
                    className="select mono"
                    id="ed-isolation"
                    name="isolation"
                    defaultValue={h.isolation || "worktree"}
                    disabled
                  >
                    <option>worktree</option>
                    <option>branch</option>
                    <option>none</option>
                  </select>
                </div>
                <div className="field">
                  <label htmlFor="ed-scope">Scope</label>
                  <select
                    className="select mono"
                    id="ed-scope"
                    name="scope"
                    defaultValue={h.scope || "warn"}
                    disabled
                  >
                    <option>off</option>
                    <option>warn</option>
                    <option>strict</option>
                  </select>
                </div>
                <div className="field">
                  <label htmlFor="ed-commits">Commits</label>
                  <select
                    className="select mono"
                    id="ed-commits"
                    name="commits"
                    defaultValue={h.commits || "per-milestone"}
                    disabled
                  >
                    <option>per-milestone</option>
                    <option>per-run</option>
                    <option>manual</option>
                  </select>
                </div>
              </div>
              <label className="check">
                <input
                  type="checkbox"
                  id="ed-risk"
                  name="risk_hold"
                  defaultChecked={h.risk_hold !== "off"}
                  disabled
                />
                <span>
                  Risk hold{" "}
                  <span className="muted">
                    (hold milestones that touch migrations or config for approval)
                  </span>
                </span>
              </label>
            </div>
            <div className="modal__foot">
              <button
                type="button"
                className="btn"
                id="edit-cancel"
                onClick={() => setEditOpen(false)}
              >
                Cancel
              </button>
              <button
                type="submit"
                className="btn btn--emerald btn--solid"
                id="edit-save"
                disabled
                title="Not available yet"
              >
                Save plan
              </button>
            </div>
          </form>
        </Modal>
      </div>
    );
  };

  return (
    <section className="screen" aria-labelledby="screen-title">
      <h1 id="screen-title" className="visually-hidden">Workflows</h1>
      <ResourceView
        resource={resource}
        empty="The plan has no milestones. Write them in .orchestrate/plan.md and run `orch sync`."
      >
        {renderContent}
      </ResourceView>
    </section>
  );
}
