import { useApi } from "../../app/connection";
import type { EventsResponse } from "../../api/types";
import { ActionResult } from "../../components/ActionResult";
import { EventList } from "../../components/EventList";
import { useAction } from "../../components/useAction";

/** The lead's queue: new events, events still open, and milestones ready for dispatch. */
export function LeadQueue({ events, onChange }: { events: EventsResponse; onChange: () => void }) {
  const api = useApi();
  const { outcome, run, pending } = useAction(onChange);
  return (
    <section className="panel" aria-label="Lead's queue">
      <h2 className="panel-title">Lead's queue</h2>
      <p className="muted">{events.lead_on ? "The lead is on." : "The lead is off: nothing is dispatched for you."}</p>
      <h3>New ({events.new.length})</h3>
      <EventList events={events.new} empty="Nothing new." />
      <h3>Still open ({events.still_open.length})</h3>
      <EventList events={events.still_open} empty="Nothing is waiting on an earlier event." />
      <h3>Ready for dispatch ({events.ready.length})</h3>
      {events.ready.length === 0 ? (
        <p className="muted">No milestone is ready.</p>
      ) : (
        <ul className="plain-list">
          {events.ready.map((r) => (
            <li key={r.task}>
              <strong>{r.task}</strong> {r.title}
              <span className="muted">
                {r.tier ? ` · ${r.tier}` : ""}
                {r.workers.length ? ` · can go to ${r.workers.join(", ")}` : ""}
              </span>{" "}
              <button
                type="button"
                className="button"
                disabled={pending}
                onClick={() => void run(() => api.dispatch({ ids: [r.task] }))}
              >
                Dispatch {r.task}
              </button>
            </li>
          ))}
        </ul>
      )}
      <ActionResult outcome={outcome} />
    </section>
  );
}
