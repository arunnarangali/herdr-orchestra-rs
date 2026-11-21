import { createContext, useContext, useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { useConnection } from "../app/connection";
import type { AttentionAction } from "../api/types";
import { useProjects } from "../app/projects";
import { useAttentionStore, type AttentionSource, type AttentionStore, type TaggedItem } from "./attentionStore";
import { NOTIFICATIONS_PATH } from "../app/paths";
import { Button } from "../ui/Button";
import { Drawer } from "../ui/Drawer";
import { Icon } from "../ui/Icon";
import { Modal } from "../ui/Modal";
import { AttentionCard } from "./AttentionCard";

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
      <NotificationDrawer />
    </AttentionContext.Provider>
  );
}

export function AttentionPopup() {
  const { currentItem, dismissCurrent, clientFor, resolveItem } = useAttention();
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

  const runAction = async (fn: () => Promise<unknown>, resolution: string) => {
    setPending(true);
    setError(null);
    try {
      await fn();
      resolveItem(item, resolution);
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
    void runAction(() => client.answer(item.task!, { text: answer }), "Answered");
  };

  const handleApprove = () => {
    // A plan is approved by telling the lead to go; a milestone is approved through its task.
    if (item.kind === "plan_approval" || !item.task) {
      void runAction(() => client.sendChat({ kind: "plan_approval", text: "", request: null }), "Approved");
      return;
    }
    void runAction(() => client.approve(item.task!), "Approved");
  };

  const handleFix = () => {
    if (!item.task) return;
    if (fix.trim() === "") {
      setError("Please describe the fix first.");
      return;
    }
    void runAction(() => client.fix(item.task!, { text: fix }), "Fix requested");
  };

  const handleDispatch = () => {
    if (!item.task) return;
    void runAction(() => client.dispatch({ ids: [item.task!] }), "Retried");
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

/** The top bar's bell: how many items wait for the developer, and the way to the drawer. */
export function NotificationBell() {
  const { waitingItems, openList } = useAttention();
  const count = waitingItems.length;
  return (
    <button
      type="button"
      className="topbar__icon-btn"
      aria-haspopup="dialog"
      aria-label={count === 0 ? "Notifications" : `Notifications, ${count} need attention`}
      onClick={openList}
    >
      <Icon name="bell" />
      {count > 0 && (
        <span className="bell__count" aria-hidden="true">
          {count}
        </span>
      )}
    </button>
  );
}

/** The notification drawer: every item that waits, with a link to the Attention Center. */
export function NotificationDrawer() {
  const { waitingItems, isListOpen, closeList, openItem } = useAttention();
  const navigate = useNavigate();
  return (
    <Drawer
      open={isListOpen}
      title="Notifications"
      icon="bell"
      onClose={closeList}
      footer={
        <Button
          block
          onClick={() => {
            closeList();
            navigate(NOTIFICATIONS_PATH);
          }}
        >
          Open Attention Center
          <Icon name="external" />
        </Button>
      }
    >
      {waitingItems.length === 0 ? (
        <p className="muted drawer__empty">All clear. Nothing needs your attention.</p>
      ) : (
        <ul className="att-list" aria-label="Waiting items">
          {waitingItems.map((item) => (
            <AttentionCard key={item.uid} item={item} onReview={openItem} />
          ))}
        </ul>
      )}
    </Drawer>
  );
}
