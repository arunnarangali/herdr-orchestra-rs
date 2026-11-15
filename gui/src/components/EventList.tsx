import type { EventInfo } from "../api/types";
import { formatTime } from "./format";
import "./components.css";

/** Lead events: what happened, to which milestone, and what to do. Text is shown as plain text. */
export function EventList({ events, empty }: { events: EventInfo[]; empty: string }) {
  if (events.length === 0) return <p className="muted">{empty}</p>;
  return (
    <ul className="event-list">
      {events.map((e) => (
        <li key={e.id} className={e.read ? "event" : "event event--unread"}>
          <div>
            <span className="event-kind">{e.kind}</span>
            {e.task && <span className="event-task"> {e.task}</span>}
            {e.task_title && <span className="muted"> {e.task_title}</span>}
            {!e.read && <span className="badge badge--info"> new</span>}
          </div>
          <div>{e.text}</div>
          {e.help && e.open && <div className="muted">{e.help}</div>}
          <time className="muted" dateTime={new Date(e.ts * 1000).toISOString()}>
            {formatTime(e.ts)}
          </time>
        </li>
      ))}
    </ul>
  );
}
