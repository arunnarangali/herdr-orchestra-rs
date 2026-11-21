import { useApi } from "../../app/connection";
import type { EventInfo, EventsResponse } from "../../api/types";
import { ActionResult } from "../../components/ActionResult";
import { useAction } from "../../components/useAction";
import { ExecIcon } from "./icons";
import { timeOfDay } from "./time";

function toneOf(kind: string): string {
  const k = kind.toLowerCase();
  if (/(fail|conflict|held|broken|lost|stuck)/.test(k)) return "coral";
  if (/(answer|question|review|paused)/.test(k)) return "amber";
  if (/(merged|done|passed|committed|ready)/.test(k)) return "emerald";
  return "cyan";
}

function nameOf(kind: string): string {
  return kind
    .split(/[._-]/)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join("");
}

/** Newest last, the way a timeline reads; the events the lead has not read yet come with the open ones. */
export function timelineOf(events: EventsResponse): EventInfo[] {
  const seen = new Set<number>();
  return [...events.still_open, ...events.new]
    .filter((e) => (seen.has(e.id) ? false : (seen.add(e.id), true)))
    .sort((a, b) => a.ts - b.ts || a.id - b.id)
    .slice(-12);
}

const WHY_NO_ACK = "Not available yet: the API cannot mark an event as read.";

/**
 * The lead's queue as the prototype's timeline: one row per event with Acknowledge, Replay and
 * Open, then the milestones ready for dispatch. Acknowledge and Replay have no API call yet, so
 * they are shown disabled with the reason; nothing here pretends to have done them.
 */
export function LeadQueue({
  events,
  stateLabel,
  stateTone,
  onOpen,
  onChange,
}: {
  events: EventsResponse;
  stateLabel: string;
  stateTone: string;
  onOpen: (task: string) => void;
  onChange: () => void;
}) {
  const api = useApi();
  const { outcome, run, pending } = useAction(onChange);
  const items = timelineOf(events);
  return (
    <section className="card timeline" aria-label="Lead's queue">
      <header className="card__head">
        <h2 className="card__title">
          <ExecIcon name="layers" />
          Lead Event Queue &amp; Transition Timeline
        </h2>
        <span className={`badge badge--pill badge--${stateTone}`}>
          <span className={`dot dot--${stateTone}`} aria-hidden="true" />
          {stateLabel} ({events.new.length} new)
        </span>
      </header>
      <p className="muted tl__note" id="tl-why">
        {events.lead_on ? "The lead is on." : "The lead is off: nothing is dispatched for you."} Acknowledge and Replay are not
        available yet: the API cannot do them.
      </p>

      {items.length === 0 ? (
        <p className="tl__empty">Nothing new.</p>
      ) : (
        <ol className="tl">
          {items.map((e) => {
            const name = nameOf(e.kind);
            const tone = toneOf(e.kind);
            return (
              <li key={e.id} className="tl__item" style={{ ["--c" as string]: `var(--accent-${tone})` }}>
                <span className={`dot dot--${tone}`} aria-hidden="true" />
                <time className="tl__time" dateTime={new Date(e.ts * 1000).toISOString()}>
                  {timeOfDay(e.ts)}
                </time>
                <div>
                  <div className="tl__name">
                    {name}
                    {e.task && <span className="mono"> · {e.task}</span>}
                    {e.task_title && <span className="muted"> {e.task_title}</span>}
                    {!e.read && <span className="badge badge--info"> new</span>}
                  </div>
                  <div className="tl__msg">{e.text}</div>
                  {e.help && e.open && <div className="tl__msg muted">{e.help}</div>}
                </div>
                <div className="tl__act">
                  {e.read ? (
                    <span className="tl__done">
                      <ExecIcon name="check" size={14} />
                      Read
                    </span>
                  ) : (
                    <button
                      type="button"
                      className="btn btn--emerald btn--sm"
                      disabled
                      aria-describedby="tl-why"
                      aria-label={`Acknowledge ${name}`}
                      title={WHY_NO_ACK}
                    >
                      Ack <ExecIcon name="check" size={13} />
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn btn--icon btn--sm"
                    disabled
                    aria-describedby="tl-why"
                    aria-label={`Replay ${name} to lead`}
                    title="Not available yet"
                  >
                    <ExecIcon name="sync" />
                  </button>
                  <button
                    type="button"
                    className="btn btn--icon btn--sm"
                    disabled={!e.task}
                    aria-label={e.task ? `Open ${e.task}` : `Open ${name} (no milestone)`}
                    title={e.task ? `Open ${e.task}` : "This event has no milestone"}
                    onClick={() => e.task && onOpen(e.task)}
                  >
                    <ExecIcon name="external" />
                  </button>
                </div>
              </li>
            );
          })}
        </ol>
      )}

      <div className="tl__ready">
        <h3>Ready for dispatch ({events.ready.length})</h3>
        {events.ready.length === 0 ? (
          <p className="muted">No milestone is ready.</p>
        ) : (
          <ul className="plain-list">
            {events.ready.map((r) => (
              <li key={r.task} className="tl__ready-row">
                <strong>{r.task}</strong> {r.title}
                <span className="muted">
                  {r.tier ? ` · ${r.tier}` : ""}
                  {r.workers.length ? ` · can go to ${r.workers.join(", ")}` : ""}
                </span>
                <button
                  type="button"
                  className="btn btn--sm"
                  disabled={pending}
                  onClick={() => void run(() => api.dispatch({ ids: [r.task] }))}
                >
                  Dispatch {r.task}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <ActionResult outcome={outcome} />
    </section>
  );
}
