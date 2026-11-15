import { useEffect, useRef, useState } from "react";
import { LiveStatus } from "../../components/LiveStatus";
import { LeadUnavailable } from "../../components/useSendChat";
import { ErrorPanel } from "../../ui/ResourceView";
import { Bubble, OutboxBubble } from "./Bubble";
import { Composer } from "./Composer";
import { EMPTY_REQUEST, FeatureForm } from "./FeatureForm";
import { useChat } from "./useChat";
import "./chat.css";

/** The developer's conversation with the lead: messages, delivery, and the feature request form. */
export function ChatScreen() {
  const chat = useChat();
  const [formOpen, setFormOpen] = useState(false);
  const [request, setRequest] = useState(EMPTY_REQUEST);
  const end = useRef<HTMLLIElement>(null);
  const count = chat.messages.length + chat.outbox.length;
  const newest = chat.messages.at(-1)?.id;

  useEffect(() => {
    end.current?.scrollIntoView?.({ block: "end" });
  }, [count, newest, chat.outbox.length]);

  return (
    <section className="screen chat" aria-labelledby="screen-title">
      <div className="screen-head">
        <h1 id="screen-title">Chat</h1>
        <LiveStatus />
        <span className="spacer" />
        <button
          type="button"
          className="button"
          aria-expanded={formOpen}
          onClick={() => setFormOpen((o) => !o)}
        >
          New feature request
        </button>
      </div>

      {formOpen && (
        <FeatureForm
          values={request}
          onChange={setRequest}
          chat={chat}
          onCancel={() => setFormOpen(false)}
          onSent={() => {
            setRequest(EMPTY_REQUEST);
            setFormOpen(false);
          }}
        />
      )}
      {chat.leadProblem && <LeadUnavailable text={chat.leadProblem} />}

      {chat.state === "loading" && (
        <p role="status" className="muted">
          Loading the conversation…
        </p>
      )}
      {chat.state === "error" && chat.error && <ErrorPanel error={chat.error} onRetry={chat.reload} />}

      {chat.state === "ready" && (
        <>
          {count === 0 && (
            <p className="muted chat-empty">
              Nothing here yet. Write to the lead below, or send a <strong>new feature request</strong> to get a plan.
            </p>
          )}
          <ol className="chat-log" role="log" aria-label="Conversation with the lead" aria-live="polite" tabIndex={0}>
            {chat.messages.map((m) => (
              <Bubble key={m.id} message={m} onRetry={() => chat.resend(m)} />
            ))}
            {chat.outbox.map((e) => (
              <OutboxBubble key={e.key} entry={e} onRetry={() => chat.retryOutbox(e.key)} />
            ))}
            <li ref={end} aria-hidden="true" className="chat-end" />
          </ol>
        </>
      )}
      <Composer onSend={(text) => chat.sendQueued({ kind: "message", text, request: null })} />
    </section>
  );
}
