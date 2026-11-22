import { shownText } from "./featureCommand";
import type { ChatMessage } from "../../api/types";
import type { OutboxEntry } from "./useChat";

export const DELIVERY_LABEL: Record<string, string> = {
  queued: "Queued",
  sent: "Sent",
  acknowledged: "Acknowledged",
  failed: "Failed",
};
const DELIVERY_TONE: Record<string, string> = {
  queued: "muted",
  sent: "info",
  acknowledged: "success",
  failed: "danger",
};

/** `14:05` in the reader's time zone; the full time is in the tooltip. */
function Time({ ts }: { ts: string }) {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return null;
  return (
    <time dateTime={ts} title={d.toLocaleString()}>
      {d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
    </time>
  );
}

const FIELDS = [
  ["Feature", "feature"],
  ["Why", "why"],
  ["Must", "must"],
  ["Must not", "must_not"],
  ["Already decided", "decided"],
  ["Done when", "done_when"],
  ["Mode", "mode"],
] as const;

function Request({ request }: { request: NonNullable<ChatMessage["request"]> }) {
  return (
    <dl className="chat-request">
      {FIELDS.filter(([, k]) => request[k].trim() !== "").map(([label, k]) => (
        <div key={k}>
          <dt>{label}</dt>
          <dd>{request[k]}</dd>
        </div>
      ))}
    </dl>
  );
}

function Delivery({ state, note, onRetry }: { state: string; note: string | null; onRetry?: () => void }) {
  return (
    <span className="chat-delivery" data-delivery={state}>
      <span className={`badge badge--${DELIVERY_TONE[state] ?? "muted"}`}>{DELIVERY_LABEL[state] ?? state}</span>
      {note && state !== "acknowledged" && <span className="chat-note">{note}</span>}
      {state === "failed" && onRetry && (
        <button type="button" className="button chat-retry" onClick={onRetry}>
          Retry
        </button>
      )}
    </span>
  );
}

/** One stored message: the developer's on the right, the lead's on the left. */
export function Bubble({ message, onRetry }: { message: ChatMessage; onRetry: () => void }) {
  const mine = message.sender === "developer";
  const kind =
    message.kind === "feature_request" ? "Feature request" : message.kind === "plan_approval" ? "Plan approved" : null;
  return (
    <li className={`chat-row chat-row--${mine ? "mine" : "lead"}`} data-message={message.id}>
      <div className="chat-bubble">
        <p className="chat-meta">
          <span className="chat-who">{mine ? "You" : "Lead"}</span>
          {kind && <span className="badge badge--info">{kind}</span>}
          {message.about && <span className="badge badge--neutral">about {message.about}</span>}
          <Time ts={message.ts} />
        </p>
        {message.request ? <Request request={message.request} /> : <p className="chat-text">{shownText(message.text)}</p>}
        {mine && message.delivery && <Delivery state={message.delivery} note={message.delivery_note} onRetry={onRetry} />}
      </div>
    </li>
  );
}

/** A message that is on its way to the server, or never got there. */
export function OutboxBubble({ entry, onRetry }: { entry: OutboxEntry; onRetry: () => void }) {
  return (
    <li className="chat-row chat-row--mine" data-outbox={entry.state}>
      <div className="chat-bubble chat-bubble--pending">
        <p className="chat-meta">
          <span className="chat-who">You</span>
        </p>
        <p className="chat-text">{shownText(entry.body.text)}</p>
        {entry.state === "waiting" ? (
          <span className="chat-delivery" role="status">
            Waiting: it is sent as soon as the Lead has set up this project's run.
          </span>
        ) : entry.state === "sending" ? (
          <span className="chat-delivery" role="status">
            Sending…
          </span>
        ) : (
          <span className="chat-delivery" role="alert">
            <span className="badge badge--danger">Not sent</span>
            <span className="chat-note">{entry.error}</span>
            <button type="button" className="button chat-retry" onClick={onRetry}>
              Retry
            </button>
          </span>
        )}
      </div>
    </li>
  );
}
