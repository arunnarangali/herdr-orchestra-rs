import { useCallback, useEffect, useRef, useState } from "react";
import { useConnection } from "../app/connection";
import { kindKey } from "./format";
import "./components.css";

export interface Notice {
  id: number;
  tone: "warning" | "danger" | "success" | "info";
  text: string;
}

const KINDS: Record<string, { tone: Notice["tone"]; title: string }> = {
  "needs-answer": { tone: "warning", title: "Question" },
  held: { tone: "warning", title: "Held" },
  failed: { tone: "danger", title: "Failed" },
  "all-done": { tone: "success", title: "Run finished" },
  stopped: { tone: "warning", title: "Run stopped" },
};

function desktopAllowed(): boolean {
  return typeof Notification !== "undefined" && Notification.permission === "granted";
}

/**
 * In-app notices for what needs the developer: a new question, a hold, a failure, the end of
 * the run. When the app runs in the desktop shell and the user allowed it, the same text is
 * also a desktop notification.
 */
export function useNotices() {
  const { stream, shell } = useConnection();
  const [notices, setNotices] = useState<Notice[]>([]);
  const next = useRef(1);
  const { subscribe } = stream;

  useEffect(
    () =>
      subscribe((m) => {
        if (m.event !== "lead-event") return;
        const k = KINDS[kindKey(m.data.kind)];
        if (!k) return;
        const text = `${k.title}${m.data.task ? ` ${m.data.task}` : ""}: ${m.data.text}`;
        setNotices((n) => [...n.slice(-4), { id: next.current++, tone: k.tone, text }]);
        if (shell && desktopAllowed()) new Notification(k.title, { body: m.data.text });
      }),
    [subscribe, shell],
  );

  const dismiss = useCallback((id: number) => setNotices((n) => n.filter((x) => x.id !== id)), []);
  return { notices, dismiss };
}

export function NoticeList({ notices, dismiss }: { notices: Notice[]; dismiss: (id: number) => void }) {
  if (notices.length === 0) return null;
  return (
    <ul className="notices" aria-label="Notices" role="status">
      {notices.map((n) => (
        <li key={n.id} className={`notice notice--${n.tone}`}>
          <span>{n.text}</span>
          <button type="button" className="button" onClick={() => dismiss(n.id)} aria-label="Dismiss notice">
            Dismiss
          </button>
        </li>
      ))}
    </ul>
  );
}

/** Offers desktop notifications, only in the desktop shell and only until the user decides. */
export function NotificationOptIn() {
  const { shell } = useConnection();
  const [permission, setPermission] = useState(() =>
    typeof Notification === "undefined" ? "unsupported" : Notification.permission,
  );
  if (!shell || permission !== "default") return null;
  return (
    <button
      type="button"
      className="button"
      onClick={() => void Notification.requestPermission().then(setPermission)}
    >
      Allow desktop notifications
    </button>
  );
}
