import type { PlanHeader, StatusResponse } from "../../api/types";

type Row = [label: string, value: string | null | undefined, hint?: string];

function rows(h: PlanHeader, s: StatusResponse | null): Row[] {
  return [
    ["Branch", h.branch ?? s?.branch],
    ["Base", h.base ?? s?.base],
    ["Isolation", h.isolation ?? s?.isolation, "worktree: each coder works in its own folder"],
    ["Max parallel", h.max_parallel ?? (s ? String(s.max_parallel) : null), "coders working at the same time"],
    ["Scope", h.scope, "what happens when a coder changes files outside the milestone's list"],
    ["Risk hold", h.risk_hold, "risky milestones wait for your approval"],
    ["Commits", h.commits],
    ["Lead", h.lead],
    ["Lead mode", h.lead_mode ?? s?.lead_mode],
    ["Workers", h.workers],
    ["Reviewer", h.reviewer ?? s?.reviewer],
    ["Budget", h.budget],
    ["Check command", h.check_command],
    ["Setup command", h.setup_command],
    ["Setup inputs", h.setup_inputs],
  ];
}

/** The run's settings as the API reports them: read-only, they come from plan.md. */
export function RunSettings({ header, status }: { header: PlanHeader; status: StatusResponse | null }) {
  return (
    <section className="panel" aria-label="Run settings">
      <h2 className="panel-title">Run settings</h2>
      <p className="muted" role="note">
        Read-only. These come from the header of <code>plan.md</code>; change them there and run <code>orch sync</code>.
        This app cannot change them.
      </p>
      <dl className="st-facts">
        {rows(header, status).map(([label, value, hint]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>
              {value ? (
                label.endsWith("command") || label === "Setup inputs" ? <code>{value}</code> : value
              ) : (
                <span className="muted">not set</span>
              )}
              {hint && <span className="muted st-hint"> {hint}</span>}
            </dd>
          </div>
        ))}
      </dl>
      <h3 className="st-sub">Other header lines</h3>
      {header.extra.length === 0 ? (
        <p className="muted">None. The API does not report tunables or their defaults.</p>
      ) : (
        <dl className="st-facts">
          {header.extra.map(([k, v], i) => (
            <div key={`${k}-${i}`}>
              <dt>{k}</dt>
              <dd>{v}</dd>
            </div>
          ))}
        </dl>
      )}
    </section>
  );
}
