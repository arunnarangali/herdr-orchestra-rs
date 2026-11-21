import type React from "react";
import type { PlanMilestone, TaskInfo } from "../../api/types";
import { WfIcon } from "./icons";

const KW = /\b(?:pub|trait|async|fn|impl|struct|let|CREATE|TABLE|PRIMARY|KEY|NOT|NULL|UNIQUE|DEFAULT|GET|POST|PUT|DELETE)\b/;
const TY = /\b(?:Result|Vec|Uuid|UUID|TEXT|TIMESTAMPTZ|Category\w*|CreateCategory|UpdateCategory)\b/;

function highlightSyntax(line: string) {
  const cm = line.search(/(\/\/|--)/);
  const code = cm >= 0 ? line.slice(0, cm) : line;
  const comment = cm >= 0 ? line.slice(cm) : "";
  const parts: React.ReactNode[] = [];
  const re = new RegExp(`${KW.source}|${TY.source}`, "g");
  const kwOnly = new RegExp(`^${KW.source}$`);
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(code)) !== null) {
    if (m.index > last) parts.push(code.slice(last, m.index));
    parts.push(
      <span key={m.index} className={kwOnly.test(m[0]) ? "tok-kw" : "tok-ty"}>
        {m[0]}
      </span>,
    );
    last = m.index + m[0].length;
  }
  if (last < code.length) parts.push(code.slice(last));
  if (comment) parts.push(<span key="comment" className="tok-cm">{comment}</span>);
  return parts;
}

const STATE_CONFIG: Record<string, { label: string; tone: string }> = {
  merged: { label: "Merged", tone: "emerald" },
  coding: { label: "Coding", tone: "cyan" },
  verifying: { label: "Verifying", tone: "amber" },
  failed: { label: "Failed", tone: "coral" },
  pending: { label: "Pending", tone: "neutral" },
};

export function MilestoneDetail({
  milestone,
  task,
}: {
  milestone: PlanMilestone;
  task: TaskInfo | undefined;
}) {
  const state = task?.state ?? "pending";
  const st = STATE_CONFIG[state] ?? STATE_CONFIG["pending"]!;
  const busy = state === "coding" || state === "verifying";
  const lines = milestone.interfaces ? milestone.interfaces.split("\n") : [];
  const criteria = milestone.acceptance_criteria ?? [];
  const doneCount = criteria.filter((c) => (c as { met?: boolean; done?: boolean }).met || (c as { met?: boolean; done?: boolean }).done).length;

  return (
    <>
      <div className="detail__head">
        <span className="detail__id">{milestone.id}</span>
        <h2 className="detail__title" id="detail-title">
          {milestone.title}
        </h2>
        <span className={`badge ${st.tone === "neutral" ? "" : `badge--${st.tone}`}`}>
          {busy && <WfIcon name="spinner" size={13} className="spin" />}
          {st.label}
        </span>
      </div>

      <dl className="detail__facts">
        <WfIcon name="target" />
        <dt>Goal</dt>
        <dd>{milestone.goal || "—"}</dd>

        <WfIcon name="help" />
        <dt>Why</dt>
        <dd>{milestone.why || "—"}</dd>

        {milestone.why_separate && (
          <>
            <WfIcon name="help" />
            <dt>Why separate</dt>
            <dd>{milestone.why_separate}</dd>
          </>
        )}

        {milestone.depends_on.length > 0 && (
          <>
            <WfIcon name="branch" />
            <dt>Depends on</dt>
            <dd>
              {milestone.depends_on.map((d) => (
                <span key={d.id} className="path-chip">
                  {d.id}
                  {d.reason ? ` — ${d.reason}` : ""}
                </span>
              ))}
            </dd>
          </>
        )}

        {milestone.tier && (
          <>
            <WfIcon name="package" />
            <dt>Tier</dt>
            <dd>{milestone.tier}</dd>
          </>
        )}

        <WfIcon name="package" />
        <dt>Needs</dt>
        <dd>
          {milestone.needs.length > 0 ? milestone.needs.join(", ") : "—"}
        </dd>

        {milestone.model_why && (
          <>
            <WfIcon name="help" />
            <dt>Model why</dt>
            <dd>{milestone.model_why}</dd>
          </>
        )}

        {milestone.estimate && (
          <>
            <WfIcon name="package" />
            <dt>Estimate</dt>
            <dd>{milestone.estimate}</dd>
          </>
        )}

        {milestone.role && (
          <>
            <WfIcon name="user" />
            <dt>Role</dt>
            <dd>{milestone.role}</dd>
          </>
        )}

        {milestone.review && (
          <>
            <WfIcon name="checkcircle" />
            <dt>Review</dt>
            <dd>{milestone.review}</dd>
          </>
        )}

        <WfIcon name="file" />
        <dt>Likely files</dt>
        <dd>
          {milestone.likely_files.length > 0
            ? milestone.likely_files.map((f) => (
                <span key={f} className="path-chip">
                  {f}
                </span>
              ))
            : "—"}
        </dd>

        {milestone.has_open_questions && milestone.open_questions && (
          <>
            <WfIcon name="warning" />
            <dt>Open questions</dt>
            <dd>{milestone.open_questions}</dd>
          </>
        )}
      </dl>

      <div className="detail__cols">
        <div className="detail__col">
          <h3 className="detail__h">
            <WfIcon name="checkcircle" />
            Acceptance Criteria ({criteria.length})
            <span className="muted" style={{ fontWeight: 400, fontSize: 12, marginLeft: 8 }}>
              {doneCount} met
            </span>
          </h3>
          <div className="ac">
            {criteria.length === 0 ? (
              <p className="muted" style={{ fontSize: 13 }}>No acceptance criteria listed.</p>
            ) : (
              criteria.map((c) => {
                const isMet = Boolean((c as { met?: boolean; done?: boolean }).met || (c as { met?: boolean; done?: boolean }).done);
                return (
                  <button
                    key={c.id}
                    type="button"
                    className="ac__btn"
                    aria-pressed={isMet}
                    disabled
                    title="Not available yet"
                  >
                    <WfIcon name={isMet ? "checkcircle" : "circle"} />
                    <span>
                      <b>{c.id}:</b>
                      {c.text}
                    </span>
                  </button>
                );
              })
            )}
          </div>
        </div>

        <div className="detail__col">
          <h3 className="detail__h detail__h--code">
            <WfIcon name="code" />
            Interfaces (API Contract)
          </h3>
          {lines.length === 0 || (lines.length === 1 && !lines[0]?.trim()) ? (
            <p className="muted" style={{ fontSize: 13 }}>No interfaces documented in the plan.</p>
          ) : (
            <pre className="code" tabIndex={0} aria-label={`${milestone.id} interfaces`}>
              {lines.map((line, i) => (
                <span key={i} className="code__line">
                  <span className="code__no">{i + 1}</span>
                  <span>{highlightSyntax(line)}</span>
                </span>
              ))}
            </pre>
          )}
        </div>
      </div>
    </>
  );
}
