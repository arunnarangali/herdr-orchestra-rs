import { useState } from "react";
import type { ChatMessage, TaskInfo } from "../../api/types";
import { LeadUnavailable, useSendChat } from "../../components/useSendChat";
import { useResource } from "../../ui/useResource";

/** States a milestone is in before any work started; the plan is still waiting for the developer. */
const WAITING = ["pending", "needs-answer"];

const DELIVERY_TEXT: Record<string, string> = {
  queued: "waiting for the lead's pane",
  sent: "delivered to the lead",
  acknowledged: "the lead has answered",
  failed: "could not be delivered",
};

/**
 * "Approve and start": shown while the plan the lead wrote is waiting (no milestone has
 * started). The click sends a `plan_approval` message to the lead through the chat endpoint;
 * the lead starts the run. Milestones with open questions hold the approval back.
 */
export function PlanApproval({ tasks }: { tasks: TaskInfo[] }) {
  const { send, pending } = useSendChat();
  const chat = useResource((api, signal) => api.chat({}, { signal }), [], { refreshOn: ["chat"], refreshDelayMs: 100 });
  const [sent, setSent] = useState<ChatMessage | null>(null);
  const [failure, setFailure] = useState<{ kind: "lead_unavailable" | "error"; text: string } | null>(null);

  const waiting = tasks.length > 0 && tasks.every((t) => WAITING.includes(t.state));
  const stored = chat.data?.messages.filter((m) => m.kind === "plan_approval" && m.sender === "developer").at(-1) ?? null;
  const told = stored && (!sent || stored.id >= sent.id) ? stored : sent;
  if (!waiting) return null; // the run is under way: nothing left to approve

  const open = tasks.filter((t) => t.state === "needs-answer").map((t) => t.id);
  const retry = told?.delivery === "failed";

  const approve = async () => {
    setFailure(null);
    const r = await send({ kind: "plan_approval", text: "", request: null });
    if (r.ok) {
      setSent(r.message);
      chat.reload();
    } else {
      setFailure({ kind: r.kind, text: r.text });
      if (r.kind === "lead_unavailable") chat.reload();
    }
  };

  return (
    <section className="panel plan-approval" aria-labelledby="wf-approval">
      <h2 id="wf-approval" className="panel-title">
        Review the plan, then approve it
      </h2>
      <p className="muted">
        This is the plan the lead wrote. Read the milestones below; when it is right, approve it and the lead starts the run.
      </p>
      {open.length > 0 && (
        <p className="action-result action-result--error" role="note">
          {open.join(", ")} still {open.length === 1 ? "has" : "have"} open questions. Answer them with the lead first.
        </p>
      )}
      {told && !retry ? (
        <p className="action-result action-result--ok" role="status">
          Approved. The lead has been told to start the run
          {told.delivery ? ` (${DELIVERY_TEXT[told.delivery] ?? told.delivery})` : ""}.
        </p>
      ) : (
        <>
          {retry && (
            <p className="action-result action-result--error" role="alert">
              Your approval {told?.delivery_note ?? "could not be delivered"}. Send it again.
            </p>
          )}
          <button type="button" className="button button--primary" disabled={pending || open.length > 0} onClick={() => void approve()}>
            {retry ? "Approve and start again" : "Approve and start"}
          </button>
        </>
      )}
      {failure?.kind === "lead_unavailable" && <LeadUnavailable text={failure.text} />}
      {failure?.kind === "error" && (
        <p className="action-result action-result--error" role="alert">
          {failure.text}
        </p>
      )}
    </section>
  );
}
