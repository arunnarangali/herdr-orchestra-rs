import "./overview.css";
import { useState } from "react";
import { Link } from "react-router-dom";
import type { EventInfo, EventsResponse, Health, PlanResponse, StatusResponse } from "../../api/types";
import type { TaggedItem } from "../../components/attentionStore";
import { attentionLook } from "../../components/AttentionCard";
import { RunStateBadge } from "../../components/RunStateBadge";
import { Badge, Dot } from "../../ui/Badge";
import { Button, type Tone } from "../../ui/Button";
import { Card, CardHead } from "../../ui/Card";
import { Icon, type IconName } from "../../ui/Icon";
import { Segmented } from "../../ui/Segmented";
import { Table } from "../../ui/Table";

/** What the API cannot do yet: such controls are shown disabled with this reason. */
export const NOT_AVAILABLE = "Not available yet";

// ------------------------------------------------------------------------------- progress

type Group = "merged" | "coding" | "held" | "failed" | "pending" | "skipped";
const GROUP_OF: Record<string, Group> = {
  merged: "merged",
  skipped: "skipped",
  coding: "coding",
  verifying: "coding",
  "awaiting-review": "coding",
  reviewing: "coding",
  reviewed: "coding",
  committed: "coding",
  held: "held",
  "needs-answer": "held",
  "needs-fix": "held",
  paused: "held",
  failed: "failed",
  conflict: "failed",
};
const LEGEND: { group: Group; label: string; tone: Tone | "idle" }[] = [
  { group: "merged", label: "Merged", tone: "emerald" },
  { group: "coding", label: "Coding", tone: "cyan" },
  { group: "held", label: "Held", tone: "amber" },
  { group: "failed", label: "Failed", tone: "coral" },
  { group: "pending", label: "Pending", tone: "idle" },
  { group: "skipped", label: "Skipped", tone: "idle" },
];

/** Milestone counts in the overview's five groups, and the overall percentage. */
export function progressOf(status: StatusResponse) {
  const c: Record<Group, number> = { merged: 0, coding: 0, held: 0, failed: 0, pending: 0, skipped: 0 };
  for (const p of status.progress) c[GROUP_OF[p.state] ?? "pending"] += p.count;
  const total = Object.values(c).reduce((a, b) => a + b, 0);
  // Skipped milestones are done with; work in progress counts for half.
  const percent = total ? Math.round(((c.merged + c.skipped + c.coding * 0.5) / total) * 100) : 0;
  return { counts: c, total, percent };
}

function ProgressCard({ status }: { status: StatusResponse }) {
  const { counts, total, percent } = progressOf(status);
  return (
    <Card as="section" className="ov-progress" aria-labelledby="ov-h-progress">
      <CardHead title={<span id="ov-h-progress">Overall Progress</span>} icon={<Icon name="chart" />} />
      <div className="ov-pad">
        <div className="ov-progress__top">
          <strong className="ov-progress__pct">{percent}%</strong>
          <div className="pbar" role="progressbar" aria-label="Overall progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
            {LEGEND.map(({ group }) =>
              counts[group] ? <i key={group} className={`pbar__seg pbar__seg--${group}`} style={{ flex: counts[group] }} /> : null,
            )}
          </div>
        </div>
        <ul className="legend" aria-label="Milestones by state">
          {LEGEND.filter((l) => counts[l.group] > 0 || ["merged", "coding", "pending"].includes(l.group)).map((l) => (
            <li key={l.group} className="legend__item">
              <Dot tone={l.tone} />
              {counts[l.group]} {l.label}
            </li>
          ))}
        </ul>
        <p className="muted ov-small">
          {counts.merged} of {total} merged
        </p>
      </div>
    </Card>
  );
}

function Row({ icon, label, children }: { icon: IconName; label: string; children?: React.ReactNode }) {
  return (
    <div className="mrow">
      <Icon name={icon} />
      <span className="mrow__k">{label}</span>
      {children !== undefined && <span className="mrow__v">{children}</span>}
    </div>
  );
}

function LeadCard({ status, control }: { status: StatusResponse; control: string | null }) {
  return (
    <Card as="section" aria-labelledby="ov-h-lead">
      <CardHead title={<span id="ov-h-lead">Lead State</span>} icon={<Icon name="robot" />} />
      <div className="ov-pad">
        <div className="ov-badges ov-lg">
          <RunStateBadge runState={status.run_state} control={control} />
          {status.stuck && <Badge tone="amber">Needs you</Badge>}
        </div>
        {status.broken_branch && (
          <p className="error-text" role="alert">
            The run branch is broken: {status.broken_branch.reason}
          </p>
        )}
        <Row icon="keyboard" label="Lead Mode:">
          <span className="mono text-violet">{status.lead_mode}</span>
        </Row>
        <Row icon="bell" label="Unread Events:">
          <Badge tone={status.events.unread ? "violet" : "neutral"} mono>
            {status.events.unread}
          </Badge>
          <span className="muted"> {status.events.open} open</span>
        </Row>
      </div>
    </Card>
  );
}

function WorkersCard({ status, plan }: { status: StatusResponse; plan: PlanResponse | null }) {
  const running = status.workers.filter((w) => w.task !== null);
  const budget = plan?.header.budget;
  return (
    <Card as="section" aria-labelledby="ov-h-workers">
      <CardHead title={<span id="ov-h-workers">Active Workers</span>} icon={<Icon name="robot" />} />
      <div className="ov-pad">
        <div className="slots">
          <strong>
            {running.length} / {status.max_parallel}
          </strong>
          <span>Slots Busy</span>
        </div>
        <Row icon="grid" label={`${status.isolation === "worktree" ? "Worktree" : status.isolation} Isolation`} />
        {budget && budget !== "none" && (
          <Row icon="activity" label="Budget:">
            <strong className="mono">{budget}</strong>
          </Row>
        )}
        {running.length === 0 ? (
          <p className="muted ov-small">No worker is on a milestone right now.</p>
        ) : (
          <ul className="ov-workers" aria-label="Busy workers">
            {running.map((w) => (
              <li key={w.name}>
                <span className="mono text-violet">{w.name}</span>
                <Badge mono>{w.task}</Badge>
                <span className="muted ov-small">
                  {w.model ?? "–"}
                  {w.tier ? ` · ${w.tier}` : ""}
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Card>
  );
}

function HealthCard({ status, health }: { status: StatusResponse; health: Health | null }) {
  const em = status.event_manager;
  const [tone, label]: [Tone, string] = status.broken_branch
    ? ["coral", "Broken branch"]
    : !em.running
      ? ["amber", "Driver stopped"]
      : ["emerald", "Healthy"];
  return (
    <Card as="section" aria-labelledby="ov-h-health">
      <CardHead title={<span id="ov-h-health">System Health</span>} icon={<Icon name="pulse" />} />
      <div className="ov-pad">
        <div className="ov-lg">
          <Badge tone={tone} pill>
            <Dot tone={tone} />
            {label}
          </Badge>
        </div>
        <Row icon="branch" label={health ? `orch serve v${health.version} connected` : "orch serve connected"} />
        <Row icon="bolt" label={`Event manager ${em.running ? `running${em.holder ? ` (${em.holder})` : ""}` : "not running"}`} />
      </div>
    </Card>
  );
}

function GitRow({ icon, label, children }: { icon: IconName; label: string; children: React.ReactNode }) {
  return (
    <div className="git__row">
      <Icon name={icon} />
      <dt className="git__k">{label}</dt>
      <dd className="git__v">{children}</dd>
    </div>
  );
}

/** A value the run shows that only the CLI can change for now. */
function Locked({ tone, children }: { tone: Tone; children: React.ReactNode }) {
  return (
    <button type="button" className={`badge badge--${tone} badge--mono ov-locked`} disabled title={`${NOT_AVAILABLE}: change it in the plan`}>
      {children}
      <span className="visually-hidden"> ({NOT_AVAILABLE})</span>
    </button>
  );
}

function GitCard({ status, plan }: { status: StatusResponse; plan: PlanResponse | null }) {
  const risk = plan?.header.risk_hold ?? null;
  const riskOn = risk !== null && !["off", "no", "false", "none", ""].includes(risk.toLowerCase());
  const scope = plan?.header.scope ?? null;
  return (
    <Card as="section" aria-labelledby="ov-h-git">
      <CardHead title={<span id="ov-h-git">Active Orchestration &amp; Git</span>} icon={<Icon name="branch" />} />
      <dl className="git">
        <GitRow icon="chart" label="Plan">
          <Link className="mono text-violet git__link" to="/workflows">
            {plan?.title ?? status.title}
          </Link>
        </GitRow>
        <GitRow icon="branch" label="Feature Branch">
          <span className="git__split">
            <Link className="mono text-cyan git__link" to="/execution">
              {status.branch}
            </Link>
            {status.base && <Badge mono>Base: {status.base}</Badge>}
          </span>
        </GitRow>
        <GitRow icon="activity" label="Last Commit">
          <span className="muted">{NOT_AVAILABLE}</span>
        </GitRow>
        <GitRow icon="check" label="Setup Fingerprint">
          <span className="muted">{NOT_AVAILABLE}</span>
        </GitRow>
        <GitRow icon="warning" label="Risk Hold">
          {plan ? <Locked tone={riskOn ? "coral" : "neutral"}>{riskOn ? "ON" : "OFF"}</Locked> : <span className="muted">–</span>}
        </GitRow>
        <GitRow icon="warning" label="Scope">
          {scope ? <Locked tone={scope === "off" ? "neutral" : scope === "block" ? "coral" : "amber"}>{scope}</Locked> : <span className="muted">–</span>}
        </GitRow>
      </dl>
    </Card>
  );
}

// ------------------------------------------------------------------------------- attention

const NEEDS_YOU: Record<string, { word: string; tone: Tone; icon: IconName }> = {
  held: { word: "Held", tone: "amber", icon: "warning" },
  "needs-answer": { word: "Question", tone: "cyan", icon: "question" },
  paused: { word: "Paused", tone: "amber", icon: "warning" },
  "needs-fix": { word: "Needs fix", tone: "amber", icon: "warning" },
  failed: { word: "Failed", tone: "coral", icon: "warning" },
  conflict: { word: "Conflict", tone: "coral", icon: "warning" },
};
const KIND_WORD: Record<string, string> = {
  held: "Held",
  blocked: "Blocked",
  question: "Question",
  plan_approval: "Plan approval",
  failed: "Failed",
  wake_unread: "Unread",
};

export interface AttentionHandlers {
  /** Items waiting for the developer in this project. */
  items: TaggedItem[];
  /** Open the item where it is answered (the attention popup). */
  onReview: (item: TaggedItem) => void;
  /** Approve a held milestone. Rejects with the reason when it did not work. */
  onApprove: (item: TaggedItem) => Promise<void>;
}

function AttentionRow({ item, handlers }: { item: TaggedItem; handlers: AttentionHandlers }) {
  const { tone, icon } = attentionLook(item.kind);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const approve = item.actions.includes("approve") && item.task !== null;
  return (
    <Card as="li" tone={tone} className="attn">
      <span className="attn__icon">
        <Icon name={icon} size={20} />
      </span>
      <span className="attn__text">
        <Badge tone={tone} mono className="attn__tag">
          {item.task ? `${item.task} ` : ""}
          {KIND_WORD[item.kind] ?? item.kind}
        </Badge>
        <span className="attn__body">
          — {item.agent && item.kind === "question" ? `${item.agent} asks: ` : ""}
          {item.detail ?? item.prompt ?? item.title}
        </span>
        {error && (
          <span className="error-text" role="alert">
            {error}
          </span>
        )}
      </span>
      {approve ? (
        <Button
          size="sm"
          tone={tone}
          disabled={busy}
          onClick={() => {
            setBusy(true);
            setError(null);
            handlers.onApprove(item).catch((e: unknown) => {
              setError(e instanceof Error ? e.message : String(e));
              setBusy(false);
            });
          }}
        >
          <Icon name="check" size={14} />
          Approve
        </Button>
      ) : (
        <Button size="sm" tone={tone} aria-haspopup="dialog" onClick={() => handlers.onReview(item)}>
          <Icon name={item.kind === "question" ? "chat" : "external"} size={14} />
          {item.kind === "question" ? "Answer" : "Review"}
        </Button>
      )}
    </Card>
  );
}

function AttentionCardList({ status, attention }: { status: StatusResponse; attention: AttentionHandlers | null }) {
  const items = attention?.items ?? [];
  const covered = new Set(items.map((i) => i.task).filter(Boolean));
  // Milestones that need a person but have no attention item (yet): shown from the status.
  const fromStatus = status.tasks.filter((t) => NEEDS_YOU[t.state] && !covered.has(t.id));
  const count = items.length + fromStatus.length;
  return (
    <Card as="section" className="attn-card" aria-labelledby="ov-h-attn">
      <CardHead title={<span id="ov-h-attn">Needs Attention (Action Required)</span>} icon={<Icon name="warning" />}>
        <Badge pill>
          {count} item{count === 1 ? "" : "s"}
        </Badge>
      </CardHead>
      {count === 0 ? (
        <div className="ov-empty">
          <Icon name="check" size={28} />
          <strong>All clear</strong>
          <span className="muted">No holds, questions or failures need you right now.</span>
        </div>
      ) : (
        <ul className="attn-list">
          {attention && items.map((i) => <AttentionRow key={i.uid} item={i} handlers={attention} />)}
          {fromStatus.map((t) => {
            const look = NEEDS_YOU[t.state]!;
            return (
              <Card as="li" tone={look.tone} className="attn" key={t.id}>
                <span className="attn__icon">
                  <Icon name={look.icon} size={20} />
                </span>
                <span className="attn__text">
                  <Badge tone={look.tone} mono className="attn__tag">
                    {t.id} {look.word}
                  </Badge>
                  <span className="attn__body">— {t.reason || t.title}</span>
                </span>
                <Link className={`btn btn--sm btn--${look.tone}`} to={`/execution?task=${encodeURIComponent(t.id)}`}>
                  <Icon name="external" size={14} />
                  Open
                </Link>
              </Card>
            );
          })}
        </ul>
      )}
    </Card>
  );
}

// ------------------------------------------------------------------------------- events

function eventLook(kind: string): { icon: IconName; tone: Tone } {
  const k = kind.toLowerCase();
  if (/merged|passed|done|approved/.test(k)) return { icon: "check", tone: "emerald" };
  if (/fail|conflict|broken|stopped|lost/.test(k)) return { icon: "warning", tone: "coral" };
  if (/answer|question/.test(k)) return { icon: "question", tone: "amber" };
  if (/held|hold|fix/.test(k)) return { icon: "warning", tone: "amber" };
  if (/wake|lead/.test(k)) return { icon: "bolt", tone: "violet" };
  return { icon: "activity", tone: "cyan" };
}

const stamp = (ts: number) => {
  const d = new Date(ts * 1000);
  return `${d.toLocaleDateString("en-US", { month: "short", day: "numeric" })}, ${d.toLocaleTimeString("en-GB", { hour12: false })}`;
};

function EventsCard({ events }: { events: EventInfo[] }) {
  const [filter, setFilter] = useState<"all" | "unread">("all");
  const unread = events.filter((e) => !e.read).length;
  const rows = filter === "unread" ? events.filter((e) => !e.read) : events;
  return (
    <Card as="section" className="ev" aria-labelledby="ov-h-events">
      <CardHead title={<span id="ov-h-events">Recent Execution Events</span>} icon={<Icon name="activity" />}>
        <Segmented
          label="Event filter"
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: "All" },
            { value: "unread", label: `Unread (${unread})` },
          ]}
        />
        <Button size="sm" unavailable={`${NOT_AVAILABLE}: the lead marks events read`}>
          <Icon name="check" size={14} />
          Ack all
        </Button>
        <Badge>Last {events.length || 50} events</Badge>
      </CardHead>
      <Table caption="Recent execution events" className="ev__table">
        <thead>
          <tr>
            {["#", "Timestamp", "Event", "Milestone", "Worker", "Details", "Ack"].map((c) => (
              <th key={c} scope="col">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 ? (
            <tr>
              <td colSpan={7} className="ov-empty-cell">
                {events.length === 0 ? "No events yet." : "Every event is read."}
              </td>
            </tr>
          ) : (
            rows.map((e) => {
              const look = eventLook(e.kind);
              return (
                <tr key={e.id} className={e.read ? "is-read" : undefined}>
                  <td className="mono muted">{String(e.id).padStart(3, "0")}</td>
                  <td className="mono ev__ts">
                    <time dateTime={new Date(e.ts * 1000).toISOString()}>{stamp(e.ts)}</time>
                  </td>
                  <td>
                    <span className={`ev__name text-${look.tone}`}>
                      <Icon name={look.icon} />
                      <span className="mono">{e.kind}</span>
                    </span>
                  </td>
                  <td>{e.task ? <Badge mono>{e.task}</Badge> : <span className="muted">–</span>}</td>
                  <td className="mono text-violet">{e.agent ?? <span className="muted">–</span>}</td>
                  <td className="ev__detail">
                    {e.text}
                    {e.help && e.open && <div className="muted ov-small">{e.help}</div>}
                  </td>
                  <td className="ev__ack">
                    {e.read ? (
                      <span className="ev__done">
                        <Icon name="check" size={14} />
                        Read
                      </span>
                    ) : (
                      <Button size="sm" tone="emerald" unavailable={NOT_AVAILABLE} aria-label={`Acknowledge event ${e.id} ${e.kind}`}>
                        <Icon name="check" size={14} />
                        Ack
                      </Button>
                    )}
                  </td>
                </tr>
              );
            })
          )}
        </tbody>
      </Table>
    </Card>
  );
}

// ------------------------------------------------------------------------------- the screen body

export interface RunOverviewProps {
  status: StatusResponse;
  events: EventsResponse | null;
  /** The run's control state from the live stream, when known. */
  control?: string | null;
  plan?: PlanResponse | null;
  health?: Health | null;
  attention?: AttentionHandlers | null;
}

/** The newest events first, each once (an event can be both new and still open), at most 50. */
export function recentEvents(events: EventsResponse | null): EventInfo[] {
  if (!events) return [];
  const seen = new Set<number>();
  return [...events.new, ...events.still_open]
    .filter((e) => !seen.has(e.id) && seen.add(e.id))
    .sort((a, b) => b.ts - a.ts || b.id - a.id)
    .slice(0, 50);
}

/** The overview of one run: summary cards, what needs the developer, and the recent events. */
export function RunOverview({ status, events, control = null, plan = null, health = null, attention = null }: RunOverviewProps) {
  return (
    <>
      <div className="ov-top">
        <ProgressCard status={status} />
        <LeadCard status={status} control={control} />
        <WorkersCard status={status} plan={plan} />
        <HealthCard status={status} health={health} />
      </div>
      <div className="ov-mid">
        <GitCard status={status} plan={plan} />
        <AttentionCardList status={status} attention={attention} />
      </div>
      <EventsCard events={recentEvents(events)} />
    </>
  );
}
