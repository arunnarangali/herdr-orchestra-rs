import { useResource } from "../../ui/useResource";
import { ErrorPanel } from "../../ui/ResourceView";
import { LiveStatus } from "../../components/LiveStatus";
import { NoticeList, NotificationOptIn, useNotices } from "../../components/Notices";
import { useRunControl } from "../../components/useRunControl";
import { RunOverview } from "./RunOverview";
import "./overview.css";

/** Where the run stands at a glance. Re-reads when the live stream reports a change. */
export function OverviewScreen() {
  const status = useResource((api, signal) => api.status({ signal }), [], {
    refreshOn: ["task-state", "run-state", "lead-event"],
  });
  const events = useResource((api, signal) => api.events({ limit: 20 }, { signal }), [], {
    refreshOn: ["lead-event", "task-state"],
  });
  const { control } = useRunControl();
  const { notices, dismiss } = useNotices();

  return (
    <section className="screen overview" aria-labelledby="screen-title">
      <div className="screen-head">
        <h1 id="screen-title">Overview</h1>
        <LiveStatus />
        <NotificationOptIn />
      </div>
      <NoticeList notices={notices} dismiss={dismiss} />
      {status.state === "loading" && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {status.state === "error" && status.error?.isNoRun && (
        <div className="panel" role="status">
          <p className="panel-title">No run in this project yet</p>
          <p className="muted">Write .orchestrate/plan.md and run `orch sync`; this page fills in by itself.</p>
        </div>
      )}
      {status.state === "error" && !status.error?.isNoRun && status.error && (
        <ErrorPanel error={status.error} onRetry={status.reload} />
      )}
      {status.data && <RunOverview status={status.data} events={events.data} control={control} />}
    </section>
  );
}
