import { createContext, useContext, useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useConnection } from "../app/connection";
import type { AttentionAction } from "../api/types";
import { useProjects } from "../app/projects";
import { useAttentionStore, type AttentionSource, type AttentionStore, type TaggedItem } from "./attentionStore";
import { Modal } from "../ui/Modal";

type AttentionContextValue = Omit<AttentionStore, "watchers">;

const AttentionContext = createContext<AttentionContextValue | null>(null);

export function useAttention() {
  const ctx = useContext(AttentionContext);
  if (!ctx) {
    throw new Error("useAttention must be used within an AttentionProvider");
  }
  return ctx;
}

/**
 * The attention popup, indicator and list. With several open projects the items of every one
 * of them come from the project list's own store (they must survive switching projects);
 * with one server this reads that server.
 */
export function AttentionProvider({ children }: { children: ReactNode }) {
  const { client, stream } = useConnection();
  const { multi, attention } = useProjects();
  const own = useMemo<AttentionSource[]>(
    () => (!multi && client ? [{ key: "", name: null, client, subscribe: stream.subscribe }] : []),
    [multi, client, stream.subscribe],
  );
  const local = useAttentionStore(own);
  const store = multi && attention ? attention : local;
  const { watchers, ...value } = store;
  return (
    <AttentionContext.Provider value={value}>
      {!multi && watchers}
      {children}
      <AttentionPopup />
      <AttentionListModal />
    </AttentionContext.Provider>
  );
}

export function AttentionPopup() {
  const { currentItem, dismissCurrent, clientFor } = useAttention();
  const { select } = useProjects();
  const navigate = useNavigate();

  const answerId = useId();
  const fixId = useId();

  const [answer, setAnswer] = useState("");
  const [fix, setFix] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset fields when currentItem changes
  useEffect(() => {
    setAnswer("");
    setFix("");
    setError(null);
    setPending(false);
  }, [currentItem?.uid]);

  // The only client that may act on an item is its own project's.
  const client = currentItem ? clientFor(currentItem.sourceKey) : null;
  if (!currentItem || !client) return null;

  const item: TaggedItem = currentItem;
  const actions: AttentionAction[] = (item.actions ?? []) as AttentionAction[];

  const runAction = async (fn: () => Promise<unknown>) => {
    setPending(true);
    setError(null);
    try {
      await fn();
      dismissCurrent();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPending(false);
    }
  };

  const handleAnswer = () => {
    if (!item.task) return;
    if (answer.trim() === "") {
      setError("Please write an answer first.");
      return;
    }
    void runAction(() => client.answer(item.task!, { text: answer }));
  };

  const handleApprove = () => {
    if (!item.task) return;
    void runAction(() => client.approve(item.task!));
  };

  const handleFix = () => {
    if (!item.task) return;
    if (fix.trim() === "") {
      setError("Please describe the fix first.");
      return;
    }
    void runAction(() => client.fix(item.task!, { text: fix }));
  };

  const handleDispatch = () => {
    if (!item.task) return;
    void runAction(() => client.dispatch({ ids: [item.task!] }));
  };

  const handleOpenChat = () => {
    dismissCurrent();
    if (item.sourceKey) select(item.sourceKey);
    navigate("/chat");
  };

  return (
    <Modal
      open={true}
      title={item.title}
      role="alertdialog"
      onClose={dismissCurrent}
    >
      <div className="attention-dialog">
        <div className="attention-meta">
          {item.projectName && (
            <span className="badge badge--neutral attention-project">Project: {item.projectName}</span>
          )}
          {item.agent && (
            <span className="badge badge--info">Agent: {item.agent}</span>
          )}
          {item.task && (
            <span className="badge badge--info">Milestone: {item.task}</span>
          )}
          <span className="badge badge--warning">{item.kind}</span>
        </div>

        {item.detail && (
          <div className="attention-detail">
            <p>{item.detail}</p>
          </div>
        )}

        {item.prompt && (
          <div className="attention-prompt">
            <strong>Prompt:</strong>
            <pre className="attention-text">{item.prompt}</pre>
          </div>
        )}

        {error && (
          <p role="alert" className="st-msg st-msg--error">
            {error}
          </p>
        )}

        <div className="attention-actions-area">
          {actions.includes("answer") && (
            <div className="field">
              <label htmlFor={answerId}>Your answer:</label>
              <textarea
                id={answerId}
                rows={3}
                value={answer}
                disabled={pending}
                onChange={(e) => setAnswer(e.target.value)}
                placeholder="Type your answer here…"
              />
              <button
                type="button"
                className="button button--primary"
                disabled={pending}
                onClick={handleAnswer}
              >
                Send answer
              </button>
            </div>
          )}

          {actions.includes("fix") && (
            <div className="field">
              <label htmlFor={fixId}>Fix instructions:</label>
              <textarea
                id={fixId}
                rows={3}
                value={fix}
                disabled={pending}
                onChange={(e) => setFix(e.target.value)}
                placeholder="Describe what to change…"
              />
              <button
                type="button"
                className="button button--primary"
                disabled={pending}
                onClick={handleFix}
              >
                Send fix
              </button>
            </div>
          )}

          <div className="modal-actions">
            {actions.includes("approve") && (
              <button
                type="button"
                className="button button--primary"
                disabled={pending}
                onClick={handleApprove}
              >
                Approve
              </button>
            )}

            {actions.includes("dispatch") && (
              <button
                type="button"
                className="button button--primary"
                disabled={pending}
                onClick={handleDispatch}
              >
                Dispatch
              </button>
            )}

            {actions.includes("open_chat") && (
              <button
                type="button"
                className="button"
                disabled={pending}
                onClick={handleOpenChat}
              >
                Open chat
              </button>
            )}

            <button
              type="button"
              className="button"
              disabled={pending}
              onClick={dismissCurrent}
            >
              Dismiss
            </button>
          </div>
        </div>
      </div>
    </Modal>
  );
}

export function AttentionIndicator() {
  const { waitingItems, openList } = useAttention();
  const count = waitingItems.length;

  if (count === 0) return null;

  return (
    <button
      type="button"
      className="attention-indicator"
      onClick={openList}
      aria-label={`Attention: ${count} item${count === 1 ? "" : "s"} waiting`}
    >
      <span className="attention-indicator-icon" aria-hidden="true">
        ⚠
      </span>
      <span className="attention-indicator-label">Attention</span>
      <span className="attention-indicator-badge">{count}</span>
    </button>
  );
}

export function AttentionListModal() {
  const { waitingItems, isListOpen, closeList, openItem } = useAttention();

  if (!isListOpen) return null;

  return (
    <Modal
      open={true}
      title={`Waiting Attention Items (${waitingItems.length})`}
      onClose={closeList}
    >
      <div className="attention-list-dialog">
        {waitingItems.length === 0 ? (
          <p className="muted">No open items need your attention.</p>
        ) : (
          <ul className="attention-items-list" aria-label="Waiting items">
            {waitingItems.map((item) => (
              <li key={item.uid} className="attention-item-row">
                <div className="attention-item-info">
                  <strong>{item.title}</strong>
                  <div className="attention-item-badges">
                    {item.projectName && (
                      <span className="badge badge--neutral attention-project">{item.projectName}</span>
                    )}
                    {item.agent && (
                      <span className="badge badge--info">{item.agent}</span>
                    )}
                    {item.task && (
                      <span className="badge badge--info">{item.task}</span>
                    )}
                    <span className="badge badge--warning">{item.kind}</span>
                  </div>
                  {item.detail && <p className="muted attention-item-snippet">{item.detail}</p>}
                </div>
                <button
                  type="button"
                  className="button button--primary button--small"
                  onClick={() => openItem(item)}
                  aria-label={`Review ${item.title}`}
                >
                  Review
                </button>
              </li>
            ))}
          </ul>
        )}

        <div className="modal-actions">
          <button type="button" className="button" onClick={closeList}>
            Close
          </button>
        </div>
      </div>
    </Modal>
  );
}
