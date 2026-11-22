import { useCallback, useEffect, useRef, useState } from "react";
import { LiveStatus } from "../../components/LiveStatus";
import { LeadUnavailable } from "../../components/useSendChat";
import { Icon } from "../../ui/Icon";
import { Modal } from "../../ui/Modal";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { AgentsAtWork } from "./AgentsAtWork";
import { AttentionInbox } from "./AttentionInbox";
import { Bubble, OutboxBubble } from "./Bubble";
import { Composer } from "./Composer";
import { EMPTY_REQUEST, FeatureForm } from "./FeatureForm";
import { PlanCard } from "./PlanCard";
import { LeadPanel } from "./LeadPanel";
import { useChat } from "./useChat";
import { useLead } from "./useLead";
import "./chat.css";

const LEAD_STATE: Record<string, { label: string; tone: string }> = {
  running: { label: "Running", tone: "emerald" },
  idle: { label: "Idle", tone: "neutral" },
  all_done: { label: "All done", tone: "emerald" },
  stopped: { label: "Stopped", tone: "neutral" },
  broken_branch: { label: "Branch broken", tone: "coral" },
};

/** The developer's conversation with the lead, its plan, what needs attention and who is working. */
export function ChatScreen() {
  const lead = useLead();
  const chat = useChat(lead);
  const ready = chat.state === "ready";
  const status = useResource(
    (api, signal) => (ready ? api.status({ signal }) : Promise.resolve(null)),
    [ready],
    { refreshOn: ["task-state", "run-state"], refreshDelayMs: 300 },
  );
  const [formOpen, setFormOpen] = useState(false);
  const [request, setRequest] = useState(EMPTY_REQUEST);
  const end = useRef<HTMLLIElement>(null);
  const count = chat.messages.length + chat.outbox.length;
  const newest = chat.messages.at(-1)?.id;
  const run = status.state === "ready" ? status.data : null;
  const leadLook = run ? LEAD_STATE[run.run_state] ?? { label: run.run_state, tone: "neutral" } : null;

  const scrollToEnd = useCallback(() => {
    end.current?.scrollIntoView?.({ block: "end" });
  }, []);

  useEffect(() => {
    scrollToEnd();
  }, [scrollToEnd, count, newest, chat.outbox.length]);

  return (
    <section className="screen chat-home" aria-labelledby="screen-title">
      <h1 id="screen-title" className="visually-hidden">
        Chat
      </h1>
      <div className="home">
        <section className="card chat" aria-labelledby="lead-title">
          <header className="chat__head">
            <span className="avatar avatar--lead" aria-hidden="true">
              <Icon name="robot" />
            </span>
            <div className="chat__who">
              <h2 className="chat__title" id="lead-title">
                Lead
              </h2>
              <p className="chat__sub">Orchestration Lead <span aria-hidden="true">·</span> Plans <span aria-hidden="true">·</span> Delegates <span aria-hidden="true">·</span> Monitors</p>
            </div>
            <LiveStatus />
            {leadLook && <span className={`badge badge--${leadLook.tone}`}>Status: {leadLook.label}</span>}
            <button
              type="button"
              className="button chat__new-request"
              aria-expanded={formOpen}
              onClick={() => setFormOpen((o) => !o)}
            >
              New feature request
            </button>
          </header>

          <LeadPanel lead={lead} />

          {chat.leadProblem && <LeadUnavailable text={chat.leadProblem} />}

          <div className="chat__thread-wrap">
            {chat.state === "loading" && (
              <p role="status" className="muted chat-state">
                Loading the conversation…
              </p>
            )}
            {chat.state === "error" && chat.error && (
              <div className="chat-state">
                <ErrorPanel error={chat.error} onRetry={chat.reload} />
              </div>
            )}

            {chat.state === "ready" && (
              <ol className="chat-log" role="log" aria-label="Conversation with the lead" aria-live="polite" tabIndex={0}>
                {count === 0 && chat.noRun && (
                  <li className="chat-empty chat-empty--calm">
                    <strong>No run in this project yet.</strong>
                    <span>
                      That is normal for a new project. Start the Lead above, or send your first message below: that
                      starts it and hands it your message.
                    </span>
                  </li>
                )}
                {count === 0 && !chat.noRun && (
                  <li className="chat-empty muted">
                    Nothing here yet. Write to the lead below, or send a <strong>new feature request</strong> to get a plan.
                  </li>
                )}
                {chat.messages.map((m) => (
                  <Bubble key={m.id} message={m} onRetry={() => chat.resend(m)} />
                ))}
                {chat.outbox.map((e) => (
                  <OutboxBubble key={e.key} entry={e} onRetry={() => chat.retryOutbox(e.key)} />
                ))}
                {ready && (
                  <li className="chat-plan">
                    <PlanCard chat={chat} onReady={scrollToEnd} />
                  </li>
                )}
                <li ref={end} aria-hidden="true" className="chat-end" />
              </ol>
            )}
          </div>

          <Composer
            onSend={(text) => chat.sendQueued({ kind: "message", text, request: null })}
            onNewRequest={() => setFormOpen(true)}
            disabled={lead.starting}
          />
        </section>

        <aside className="side-col" aria-label="Attention and agents">
          <AttentionInbox />
          {ready && <AgentsAtWork />}
        </aside>
      </div>

      <Modal open={formOpen} title="New feature request" hideTitle onClose={() => setFormOpen(false)}>
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
      </Modal>
    </section>
  );
}
