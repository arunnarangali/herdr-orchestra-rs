import type { ReactNode } from "react";
import type { PlanMilestone, TaskInfo } from "../../api/types";
import { StatusBadge } from "../../ui/StatusBadge";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="ms-detail-section">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function Text({ lines }: { lines: string }) {
  if (!lines.trim()) return <p className="muted">not in the plan</p>;
  return <p className="prewrap">{lines}</p>;
}

/** One milestone of the plan, with everything the lead wrote and how the run stands. */
export function MilestoneDetail({
  milestone,
  task,
}: {
  milestone: PlanMilestone;
  task: TaskInfo | undefined;
}) {
  return (
    <div className="ms-detail">
      <p className="ms-detail-head">
        <StatusBadge state={task?.state ?? "pending"} />
        {task ? (
          <span>
            worker <code>{task.worker ?? task.last_worker ?? "—"}</code>
            {task.model && (
              <>
                {" "}
                · model <code>{task.model}</code>
              </>
            )}
            {task.escalated && <> · escalated</>}
          </span>
        ) : (
          <span className="muted">not in the run yet</span>
        )}
      </p>

      <Section title="Goal">
        <Text lines={milestone.goal} />
      </Section>
      <Section title="Why">
        <Text lines={milestone.why} />
      </Section>
      {milestone.why_separate && (
        <Section title="Why separate">
          <p>{milestone.why_separate}</p>
        </Section>
      )}
      {milestone.depends_on.length > 0 && (
        <Section title="Depends on">
          <ul>
            {milestone.depends_on.map((d) => (
              <li key={d.id}>
                <code>{d.id}</code>
                {d.reason ? ` — ${d.reason}` : ""}
              </li>
            ))}
          </ul>
        </Section>
      )}
      <dl className="ms-facts">
        {milestone.tier && (
          <>
            <dt>Tier</dt>
            <dd>{milestone.tier}</dd>
          </>
        )}
        {milestone.needs.length > 0 && (
          <>
            <dt>Needs</dt>
            <dd>{milestone.needs.join(", ")}</dd>
          </>
        )}
        {milestone.model_why && (
          <>
            <dt>Model why</dt>
            <dd>{milestone.model_why}</dd>
          </>
        )}
        {milestone.estimate && (
          <>
            <dt>Estimate</dt>
            <dd>{milestone.estimate}</dd>
          </>
        )}
        {milestone.role && (
          <>
            <dt>Role</dt>
            <dd>{milestone.role}</dd>
          </>
        )}
        {milestone.review && (
          <>
            <dt>Review</dt>
            <dd>{milestone.review}</dd>
          </>
        )}
        <dt>Parallel</dt>
        <dd>{milestone.parallel ? "yes" : "no"}</dd>
        {!milestone.enabled && (
          <>
            <dt>Enabled</dt>
            <dd>no</dd>
          </>
        )}
      </dl>
      {milestone.likely_files.length > 0 && (
        <Section title="Likely files">
          <ul className="file-list">
            {milestone.likely_files.map((f) => (
              <li key={f}>
                <code>{f}</code>
              </li>
            ))}
          </ul>
        </Section>
      )}
      {milestone.acceptance_criteria.length > 0 && (
        <Section title="Acceptance criteria">
          <ol className="ac-list">
            {milestone.acceptance_criteria.map((c) => (
              <li key={c.id}>
                <span className="ac-id">{c.id}</span> {c.text}
              </li>
            ))}
          </ol>
        </Section>
      )}
      <Section title="Constraints">
        <Text lines={milestone.constraints} />
      </Section>
      <Section title="Interfaces">
        <Text lines={milestone.interfaces} />
      </Section>
      <Section title="Context">
        <Text lines={milestone.context} />
      </Section>
      <Section title="Out of scope">
        <Text lines={milestone.out_of_scope} />
      </Section>
      <Section title="Open questions">
        {milestone.has_open_questions ? (
          <Text lines={milestone.open_questions} />
        ) : (
          <p className="muted">none</p>
        )}
      </Section>
      {milestone.notes_from_user.trim() && (
        <Section title="Notes from user">
          <Text lines={milestone.notes_from_user} />
        </Section>
      )}
    </div>
  );
}
