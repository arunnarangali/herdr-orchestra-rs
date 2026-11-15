import { useId, useState } from "react";
import { useApi } from "../../app/connection";
import type { TaskInfo } from "../../api/types";
import { ActionResult } from "../../components/ActionResult";
import { needsHuman } from "../../components/needsHuman";
import { useAction } from "../../components/useAction";
import { ConfirmDialog } from "../../ui/ConfirmDialog";

const SETTABLE = ["pending", "paused", "failed", "skipped"] as const;

type Asking = "approve" | "escalate" | "status" | null;

/**
 * The actions for one milestone. Which ones lead depends on its state, but the server has the
 * last word: every action is sent as asked, the API's answer or refusal is shown, and only a
 * success re-reads. Approving is only ever done by a click here, after the reasons are shown.
 */
export function TaskActions({ task, onChange }: { task: TaskInfo; onChange: () => void }) {
  const api = useApi();
  const { outcome, run, pending } = useAction(onChange);
  const ids = { answer: useId(), fix: useId(), reason: useId(), status: useId() };
  const [answer, setAnswer] = useState("");
  const [fix, setFix] = useState("");
  const [status, setStatus] = useState<(typeof SETTABLE)[number]>("paused");
  const [reason, setReason] = useState("");
  const [asking, setAsking] = useState<Asking>(null);
  const [local, setLocal] = useState<string | null>(null);

  const need = needsHuman(task.state, task.reason);
  const canAnswer = task.state === "needs-answer" || task.state === "paused";
  const canDispatch = ["pending", "paused", "failed"].includes(task.state);

  const sendAnswer = async () => {
    if (answer.trim() === "") return setLocal("Write an answer first: an empty answer is not sent.");
    setLocal(null);
    if (await run(() => api.answer(task.id, { text: answer }))) setAnswer("");
  };
  const sendFix = async () => {
    if (fix.trim() === "") return setLocal("Describe the change first: an empty fix is not sent.");
    setLocal(null);
    if (await run(() => api.fix(task.id, { text: fix }))) setFix("");
  };
  const confirm = async () => {
    const what = asking;
    setAsking(null);
    if (what === "approve") await run(() => api.approve(task.id));
    else if (what === "escalate") await run(() => api.escalate(task.id, {}));
    else if (what === "status") await run(() => api.setStatus(task.id, { status, reason: reason.trim() || null }));
  };

  return (
    <section className="panel task-actions" aria-label={`Actions for ${task.id}`}>
      <h3 className="panel-title">Actions for {task.id}</h3>
      {need && (
        <p className="needs needs--big" role="note">
          <span className="badge badge--warning">{need.label}</span> {need.text}
        </p>
      )}
      <ActionResult outcome={outcome} />
      {local && (
        <p className="action-result action-result--error" role="alert">
          {local}
        </p>
      )}

      {canAnswer && (
        <div className="field">
          <label htmlFor={ids.answer}>Your answer to {task.id}</label>
          <textarea id={ids.answer} rows={3} value={answer} onChange={(e) => setAnswer(e.target.value)} />
          <p className="muted">Goes under the milestone's notes; the same coder carries on.</p>
          <div>
            <button type="button" className="button button--primary" disabled={pending} onClick={() => void sendAnswer()}>
              Send answer
            </button>
          </div>
        </div>
      )}

      <div className="button-row">
        {task.state === "held" && (
          <button type="button" className="button button--primary" disabled={pending} onClick={() => setAsking("approve")}>
            Approve…
          </button>
        )}
        {canDispatch && (
          <button type="button" className="button" disabled={pending} onClick={() => void run(() => api.dispatch({ ids: [task.id] }))}>
            {task.state === "pending" ? "Dispatch" : "Retry"} {task.id}
          </button>
        )}
        <button type="button" className="button" disabled={pending} onClick={() => setAsking("escalate")}>
          Escalate…
        </button>
      </div>

      <div className="field">
        <label htmlFor={ids.fix}>Fix: what should change in work that landed?</label>
        <textarea id={ids.fix} rows={2} value={fix} onChange={(e) => setFix(e.target.value)} />
        <div>
          <button type="button" className="button" disabled={pending} onClick={() => void sendFix()}>
            Add a follow-up fix
          </button>
        </div>
      </div>

      <div className="field field--row">
        <label htmlFor={ids.status}>Set status</label>
        <select id={ids.status} value={status} onChange={(e) => setStatus(e.target.value as typeof status)}>
          {SETTABLE.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
        <label htmlFor={ids.reason} className="visually-hidden">
          Reason
        </label>
        <input id={ids.reason} placeholder="Reason (optional)" value={reason} onChange={(e) => setReason(e.target.value)} />
        <button type="button" className="button" disabled={pending} onClick={() => setAsking("status")}>
          Set status…
        </button>
      </div>

      {asking === "approve" && (
        <ConfirmDialog
          open
          title={`Approve ${task.id}?`}
          confirmLabel="Approve"
          onConfirm={() => void confirm()}
          onCancel={() => setAsking(null)}
        >
          <p>{task.id} is held for these reasons:</p>
          <pre className="log">{task.reason || "(no reason recorded)"}</pre>
          <p>Approving lets it land on the next step.</p>
        </ConfirmDialog>
      )}
      {asking === "escalate" && (
        <ConfirmDialog
          open
          title={`Escalate ${task.id}?`}
          confirmLabel="Escalate"
          onConfirm={() => void confirm()}
          onCancel={() => setAsking(null)}
        >
          {task.id} gets one more go on a stronger model. It can only be escalated once.
        </ConfirmDialog>
      )}
      {asking === "status" && (
        <ConfirmDialog
          open
          title={`Set ${task.id} to ${status}?`}
          confirmLabel={`Set to ${status}`}
          onConfirm={() => void confirm()}
          onCancel={() => setAsking(null)}
        >
          The server decides whether {task.id} may move from {task.state} to {status}; if not, you see its reason.
        </ConfirmDialog>
      )}
    </section>
  );
}
