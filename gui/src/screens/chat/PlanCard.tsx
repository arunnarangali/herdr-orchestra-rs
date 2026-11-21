import { useEffect, useId, useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import type { PlanMilestone, TaskInfo } from "../../api/types";
import { LeadUnavailable } from "../../components/useSendChat";
import { Badge } from "../../ui/Badge";
import { Button, type Tone } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { useToast } from "../../ui/Toast";
import { useResource } from "../../ui/useResource";
import type { Chat } from "./useChat";

/** States a milestone is in before any work started; the plan is still waiting for the developer. */
const WAITING = ["pending", "needs-answer"];

const TIER_TONE: Record<string, Tone> = { small: "emerald", medium: "amber", high: "coral" };

function Tier({ milestone }: { milestone: PlanMilestone }) {
  if (!milestone.tier) return null;
  return (
    <Badge tone={TIER_TONE[milestone.tier] ?? "neutral"} mono>
      {milestone.tier}
    </Badge>
  );
}

function Milestones({ plan, tasks }: { plan: PlanMilestone[]; tasks: TaskInfo[] }) {
  const byId = new Map(tasks.map((t) => [t.id, t]));
  return (
    <ul className="plan__milestones" aria-label="Milestones">
      {plan.map((m) => {
        const task = byId.get(m.id);
        return (
          <li className="plan__milestone" key={m.id}>
            <Icon name="branch" />
            <span className="plan__id">{m.id}</span>
            <span className="plan__name">{m.title}</span>
            {task && (
              <span className="plan__state" data-state={task.state}>
                <span className="dot dot--neutral" aria-hidden="true" />
                {task.state}
              </span>
            )}
            <Tier milestone={m} />
          </li>
        );
      })}
    </ul>
  );
}

/**
 * The plan the lead wrote, in the conversation. While every milestone still waits, the
 * developer can approve it (the approval message starts the run) or ask for changes.
 */
export function PlanCard({ chat, onReady }: { chat: Chat; onReady?: () => void }) {
  const plan = useResource((api, signal) => api.plan({ signal }), [], {
    refreshOn: ["task-state", "run-state", "chat"],
    refreshDelayMs: 150,
  });
  const tasks = useResource((api, signal) => api.tasks({ signal }), [], {
    refreshOn: ["task-state", "run-state", "chat"],
    refreshDelayMs: 150,
  });
  const toast = useToast();
  const uid = useId();
  const [changesOpen, setChangesOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [failure, setFailure] = useState<{ kind: "lead_unavailable" | "error"; text: string } | null>(null);

  useEffect(() => {
    if (plan.state === "ready" && plan.data) onReady?.();
  }, [plan.state, plan.data, tasks.state, tasks.data, onReady]);

  // A missing plan is the normal state before the lead has written one; it is not an error here.
  if (plan.state !== "ready" || !plan.data) return null;

  const milestones = plan.data.milestones;
  const taskList = tasks.state === "ready" && tasks.data ? tasks.data.tasks : [];
  const waiting = taskList.length > 0 && taskList.every((t) => WAITING.includes(t.state));
  const approved = taskList.length > 0 && !waiting;
  const openQuestions = taskList.filter((t) => t.state === "needs-answer").map((t) => t.id);
  const canApprove = waiting && openQuestions.length === 0;
  const title = plan.data.title ?? "Plan";

  const approve = async () => {
    setFailure(null);
    const r = await chat.sendNow({ kind: "plan_approval", text: "", request: null });
    if (r.ok) {
      chat.adopt(r.message);
      plan.reload();
      tasks.reload();
      toast("Plan approved — the lead has been told to start", { tone: "emerald", icon: "check" });
    } else {
      setFailure({ kind: r.kind, text: r.text });
      if (r.kind === "lead_unavailable") {
        plan.reload();
        tasks.reload();
      }
    }
  };

  const sendChanges = async (e: FormEvent) => {
    e.preventDefault();
    const text = draft.trim();
    if (text === "") return;
    setFailure(null);
    const r = await chat.sendNow({ kind: "message", text, request: null });
    if (r.ok) {
      chat.adopt(r.message);
      setDraft("");
      setChangesOpen(false);
      toast("Your changes were sent to the lead", { tone: "cyan", icon: "check" });
    } else {
      setFailure({ kind: r.kind, text: r.text });
    }
  };

  return (
    <Card as="section" className="plan" aria-label={`Plan: ${title}`}>
      <div className="plan__head">
        <Icon name="cube" />
        <h3 className="plan__title">Plan: {title}</h3>
      </div>
      <div className="plan__chips">
        <span className="plan__chip">
          <Icon name="cube" />
          {milestones.length} Milestone{milestones.length === 1 ? "" : "s"}
        </span>
        {plan.data.header.isolation && (
          <span className="plan__chip plan__chip--violet">
            <Icon name="branch" />
            Isolation: <span className="mono">{plan.data.header.isolation}</span>
          </span>
        )}
        {plan.data.header.max_parallel && (
          <span className="plan__chip plan__chip--cyan">
            <Icon name="bolt" />
            Max parallel: <span className="mono">{plan.data.header.max_parallel}</span>
          </span>
        )}
      </div>

      <Milestones plan={milestones} tasks={taskList} />

      {approved ? (
        <div className="plan__actions">
          <span className="plan__approved">
            <Icon name="check" />
            Approved · autopilot is driving
          </span>
          <Link className="plan__link" to="/workflows">
            View in Workflows (Plan)
          </Link>
        </div>
      ) : (
        <div className="plan__actions">
          <Button
            tone="emerald"
            solid
            onClick={() => void approve()}
            disabled={!canApprove || chat.sending}
            title={
              openQuestions.length > 0
                ? `${openQuestions.join(", ")} still ${openQuestions.length === 1 ? "has" : "have"} open questions`
                : undefined
            }
          >
            Approve &amp; Start
          </Button>
          <Button
            aria-expanded={changesOpen}
            aria-controls={`${uid}-changes`}
            onClick={() => setChangesOpen((o) => !o)}
            disabled={chat.sending}
          >
            Ask for Changes
          </Button>
          {openQuestions.length > 0 && (
            <span className="muted plan__note">
              {openQuestions.join(", ")} still {openQuestions.length === 1 ? "has" : "have"} open questions.
            </span>
          )}
        </div>
      )}

      {changesOpen && !approved && (
        <form className="plan__changes" id={`${uid}-changes`} onSubmit={(e) => void sendChanges(e)}>
          <label className="visually-hidden" htmlFor={`${uid}-text`}>
            Describe the changes you want to the plan
          </label>
          <textarea
            id={`${uid}-text`}
            rows={3}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="e.g. Split M2 into list/read and write endpoints"
          />
          <div className="button-row">
            <Button type="submit" tone="cyan" size="sm" disabled={draft.trim() === "" || chat.sending}>
              Send to lead
            </Button>
            <Button
              ghost
              size="sm"
              onClick={() => {
                setChangesOpen(false);
                setDraft("");
              }}
            >
              Cancel
            </Button>
          </div>
        </form>
      )}

      {failure?.kind === "lead_unavailable" && <LeadUnavailable text={failure.text} />}
      {failure?.kind === "error" && (
        <p className="action-result action-result--error" role="alert">
          {failure.text}
        </p>
      )}
    </Card>
  );
}
