import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useNavigate } from "react-router-dom";
import { useConnection } from "../app/connection";
import type { AttentionAction, AttentionItem } from "../api/types";
import { getBackend } from "../screens/projects/backend";
import { Modal } from "../ui/Modal";

interface AttentionContextValue {
  waitingItems: AttentionItem[];
  currentItem: AttentionItem | null;
  dismissCurrent: () => void;
  openItem: (item: AttentionItem) => void;
  isListOpen: boolean;
  openList: () => void;
  closeList: () => void;
}

const AttentionContext = createContext<AttentionContextValue | null>(null);

export function useAttention() {
  const ctx = useContext(AttentionContext);
  if (!ctx) {
    throw new Error("useAttention must be used within an AttentionProvider");
  }
  return ctx;
}

async function notifyDesktop(item: AttentionItem) {
  const backend = getBackend();
  let enabled = false;
  if (backend) {
    try {
      const { value } = await backend.getSettings();
      enabled = value.notifications;
    } catch {
      enabled = false;
    }
  }
  if (!enabled) return;
  if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
  const body = item.prompt || item.detail || item.title;
  new Notification(item.title, { body });
}

export function AttentionProvider({ children }: { children: ReactNode }) {
  const { client, stream } = useConnection();
  const [waitingItems, setWaitingItems] = useState<AttentionItem[]>([]);
  const [popupQueue, setPopupQueue] = useState<AttentionItem[]>([]);
  const [isListOpen, setIsListOpen] = useState(false);

  const seenRef = useRef<Set<string>>(new Set());
  const previousFocusRef = useRef<HTMLElement | null>(null);

  // Load initial attention items from server
  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    client.attention().then(
      (res) => {
        if (cancelled) return;
        setWaitingItems(res.items);
        for (const it of res.items) {
          seenRef.current.add(it.id);
        }
      },
      () => {
        // Run may not be running yet or route 404
      },
    );
    return () => {
      cancelled = true;
    };
  }, [client]);

  // Subscribe to live stream attention events
  useEffect(() => {
    const off = stream.subscribe((m) => {
      if (m.event !== "attention") return;
      const { op, item } = m.data;
      if (op === "add") {
        setWaitingItems((prev) => {
          if (prev.some((x) => x.id === item.id)) return prev;
          return [...prev, item];
        });
        // If not seen yet, enqueue for modal popup and notify
        if (!seenRef.current.has(item.id)) {
          seenRef.current.add(item.id);
          // Capture current focus if queue is currently empty
          if (document.activeElement instanceof HTMLElement && !previousFocusRef.current) {
            previousFocusRef.current = document.activeElement;
          }
          setPopupQueue((q) => {
            if (q.some((x) => x.id === item.id)) return q;
            return [...q, item];
          });
          void notifyDesktop(item);
        }
      } else if (op === "remove") {
        setWaitingItems((prev) => prev.filter((x) => x.id !== item.id));
        setPopupQueue((q) => q.filter((x) => x.id !== item.id));
      }
    });
    return off;
  }, [stream]);

  // When queue becomes empty, return focus where it was
  const prevQueueLengthRef = useRef(popupQueue.length);
  useEffect(() => {
    if (prevQueueLengthRef.current > 0 && popupQueue.length === 0) {
      if (previousFocusRef.current) {
        previousFocusRef.current.focus();
        previousFocusRef.current = null;
      }
    }
    prevQueueLengthRef.current = popupQueue.length;
  }, [popupQueue.length]);

  const dismissCurrent = useCallback(() => {
    setPopupQueue((q) => q.slice(1));
  }, []);

  const openItem = useCallback((item: AttentionItem) => {
    if (document.activeElement instanceof HTMLElement && !previousFocusRef.current) {
      previousFocusRef.current = document.activeElement;
    }
    setPopupQueue((q) => {
      // Put this item at the front of the queue
      const rest = q.filter((x) => x.id !== item.id);
      return [item, ...rest];
    });
    setIsListOpen(false);
  }, []);

  const currentItem = popupQueue[0] ?? null;

  return (
    <AttentionContext.Provider
      value={{
        waitingItems,
        currentItem,
        dismissCurrent,
        openItem,
        isListOpen,
        openList: () => setIsListOpen(true),
        closeList: () => setIsListOpen(false),
      }}
    >
      {children}
      <AttentionPopup />
      <AttentionListModal />
    </AttentionContext.Provider>
  );
}

export function AttentionPopup() {
  const { currentItem, dismissCurrent } = useAttention();
  const { client } = useConnection();
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
  }, [currentItem?.id]);

  if (!currentItem || !client) return null;

  const item = currentItem;
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
              <li key={item.id} className="attention-item-row">
                <div className="attention-item-info">
                  <strong>{item.title}</strong>
                  <div className="attention-item-badges">
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
