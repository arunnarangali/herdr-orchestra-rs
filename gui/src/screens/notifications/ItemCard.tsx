import { useId, useState } from "react";
import type { ApiClient } from "../../api/client";
import type { AttentionAction } from "../../api/types";
import type { ResolvedItem, TaggedItem } from "../../components/attentionStore";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { useToast } from "../../ui/Toast";
import { kindLook } from "./kinds";

function when(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

interface ActionProps {
  item: TaggedItem;
  client: ApiClient;
  onReview: (item: TaggedItem) => void;
  onResolved: (item: TaggedItem, resolution: string) => void;
}

/** The actions the API really offers for one open item. */
function Actions({ item, client, onReview, onResolved }: ActionProps) {
  const uid = useId();
  const toast = useToast();
  const [answer, setAnswer] = useState("");
  const [fix, setFix] = useState("");
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const actions = (item.actions ?? []) as AttentionAction[];
  const plan = item.kind === "plan_approval";

  const run = async (label: string, key: string, fn: () => Promise<unknown>) => {
    setPending(key);
    setError(null);
    try {
      await fn();
      onResolved(item, label);
      toast(`${item.title}: ${label}`, { tone: kindLook(item.kind).tone, icon: "check" });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPending(null);
    }
  };

  const sendAnswer = () => {
    if (!item.task) return;
    const text = answer.trim();
    if (text === "") {
      setError("Write an answer first.");
      return;
    }
    void run("Answered", "answer", () => client.answer(item.task!, { text }));
  };

  const approve = () => {
    if (plan || !item.task) {
      void run("Approved", "approve", () => client.sendChat({ kind: "plan_approval", text: "", request: null }));
      return;
    }
    void run("Approved", "approve", () => client.approve(item.task!));
  };

  const sendFix = () => {
    if (!item.task) return;
    const text = fix.trim();
    if (text === "") {
      setError("Describe the fix first.");
      return;
    }
    void run("Fix requested", "fix", () => client.fix(item.task!, { text }));
  };

  return (
    <div className="nc-card__actions">
      {actions.includes("answer") && item.task && (
        <div className="nc-answer">
          <label className="visually-hidden" htmlFor={`${uid}-answer`}>
            Answer {item.title}
          </label>
          <input
            id={`${uid}-answer`}
            className="input"
            type="text"
            value={answer}
            onChange={(e) => setAnswer(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                sendAnswer();
              }
            }}
            placeholder="Type your answer or additional context…"
          />
          <Button tone="cyan" onClick={sendAnswer} disabled={pending !== null}>
            Send answer
          </Button>
        </div>
      )}

      {actions.includes("fix") && item.task && (
        <div className="nc-answer">
          <label className="visually-hidden" htmlFor={`${uid}-fix`}>
            Fix instructions for {item.title}
          </label>
          <input
            id={`${uid}-fix`}
            className="input"
            type="text"
            value={fix}
            onChange={(e) => setFix(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                sendFix();
              }
            }}
            placeholder="Describe what to change…"
          />
          <Button tone="cyan" onClick={sendFix} disabled={pending !== null}>
            Send fix
          </Button>
        </div>
      )}

      {actions.includes("approve") && (
        <Button tone="emerald" solid onClick={approve} disabled={pending !== null}>
          {pending === "approve" ? "Approving…" : "Approve"}
        </Button>
      )}

      {actions.includes("dispatch") && item.task && (
        <Button
          onClick={() => void run("Retried", "dispatch", () => client.dispatch({ ids: [item.task!] }))}
          disabled={pending !== null}
        >
          {pending === "dispatch" ? "Retrying…" : "Retry"}
        </Button>
      )}

      {actions.includes("open_chat") && (
        <Button onClick={() => onReview(item)} disabled={pending !== null}>
          Open chat
        </Button>
      )}

      <Button onClick={() => onReview(item)} disabled={pending !== null}>
        Review
      </Button>

      {item.kind === "held" && (
        <>
          <Button unavailable="Not available from the app yet: inspect the changed files in Herdr.">
            <Icon name="branch" />
            Inspect Diff
          </Button>
          <Button unavailable="Not available from the app yet: use `orch pause` or `orch fix` in the terminal.">
            <Icon name="x" />
            Reject / Pause
          </Button>
        </>
      )}
      {item.kind === "blocked" && (
        <Button unavailable="Not available from the app yet: answer in the agent's Herdr pane.">
          <Icon name="external" />
          Open Pane in Herdr
        </Button>
      )}
      {item.kind === "failed" && (
        <Button unavailable="Not available from the app yet: open the agent's pane in Herdr.">
          <Icon name="external" />
          Open Pane in Herdr
        </Button>
      )}
      {item.kind === "wake_unread" && (
        <Button unavailable="Not available from the app yet: wake the lead from its Herdr pane.">
          <Icon name="sync" />
          Re-Wake Lead
        </Button>
      )}

      {error && (
        <p className="action-result action-result--error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

export function OpenItemCard({
  item,
  client,
  onReview,
  onResolved,
}: ActionProps) {
  const look = kindLook(item.kind);
  const body = item.detail ?? item.prompt?.text;
  return (
    <Card as="li" tone={look.tone} className="nc-card" data-attention={item.id}>
      <div className="nc-card__head">
        <span className="nc-card__icon">
          <Icon name={look.icon} size={22} />
        </span>
        <h3 className="nc-card__title">
          <span className="mono">
            [{item.projectName ? `${item.projectName} · ` : ""}
            {item.task ?? "Plan"}
            {item.agent ? ` · ${item.agent}` : ""}]
          </span>{" "}
          <span className="nc-card__kind">{look.label}</span>
        </h3>
        <Badge tone={look.tone}>{item.kind}</Badge>
      </div>
      <p className="nc-card__body">{item.title}</p>
      {body && <p className="nc-card__body nc-card__detail">{body}</p>}
      <div className="nc-card__meta">
        <span>{when(item.since)}</span>
        <span className="mono">api/v1/attention</span>
        {item.projectName && <span className="mono">{item.projectName}</span>}
      </div>
      <Actions item={item} client={client} onReview={onReview} onResolved={onResolved} />
    </Card>
  );
}

export function ResolvedRow({ item }: { item: ResolvedItem }) {
  const look = kindLook(item.kind);
  return (
    <li className="nc-done" data-attention={item.id}>
      <Icon name="check" />
      <span>
        <strong className="mono">
          [{item.projectName ? `${item.projectName} · ` : ""}
          {item.task ?? "Plan"}]
        </strong>{" "}
        {item.title} <span className="muted">({look.label})</span> — <strong>{item.resolution}</strong>
      </span>
      <Badge tone="emerald">resolved</Badge>
      <span className="nc-done__when">{when(item.resolvedAt)}</span>
    </li>
  );
}
