import { Link } from "react-router-dom";
import { useConnection } from "../../app/connection";
import { useProjects } from "../../app/projects";
import { useAttention } from "../../components/Attention";
import { LiveStatus } from "../../components/LiveStatus";
import { NoticeList, NotificationOptIn, useNotices } from "../../components/Notices";
import { useRunControl } from "../../components/useRunControl";
import { Card } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { RunOverview, type AttentionHandlers } from "./RunOverview";
import "./overview.css";

/** Where the run stands at a glance. Re-reads when the live stream reports a change. */
export function OverviewScreen() {
  const status = useResource((api, signal) => api.status({ signal }), [], {
    refreshOn: ["task-state", "run-state", "lead-event"],
  });
  const events = useResource((api, signal) => api.events({ limit: 50 }, { signal }), [], {
    refreshOn: ["lead-event", "task-state"],
  });
  const plan = useResource((api, signal) => api.plan({ signal }), [], { refreshOn: ["run-state"] });
  const { health } = useConnection();
  const { multi, selected } = useProjects();
  const { waitingItems, openItem, clientFor } = useAttention();
  const { control } = useRunControl();
  const { notices, dismiss } = useNotices();

  // Only this project's items: the bell and the drawer show every project's.
  const here = multi ? (selected ?? "") : "";
  const attention: AttentionHandlers = {
    items: waitingItems.filter((i) => i.sourceKey === here),
    onReview: openItem,
    onApprove: async (item) => {
      const client = clientFor(item.sourceKey);
      if (!client || !item.task) throw new Error("This project is not connected.");
      await client.approve(item.task);
    },
  };

  return (
    <section className="screen ov" aria-labelledby="screen-title">
      <header className="pg-head">
        <span className="pg-head__icon">
          <Icon name="grid" />
        </span>
        <div className="pg-head__titles">
          <h1 id="screen-title" className="pg-head__title">
            Overview
          </h1>
          <p className="pg-head__sub">System &amp; Run Command Center{status.data ? ` — ${status.data.title}` : ""}</p>
        </div>
        <div className="pg-head__side">
          <LiveStatus />
          <NotificationOptIn />
        </div>
      </header>
      <NoticeList notices={notices} dismiss={dismiss} />
      {status.state === "loading" && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {status.state === "error" && status.error?.isNoRun && (
        <Card className="ov-norun" role="status">
          <Icon name="rocket" size={32} />
          <p className="panel-title">No run in this project yet</p>
          <p className="muted">
            Plan a feature with the lead, or write <code>.orchestrate/plan.md</code> and run <code>orch sync</code>. This
            page fills in by itself once the run exists.
          </p>
          <div className="ov-norun__actions">
            <Link className="btn btn--emerald btn--solid" to="/chat">
              <Icon name="chat" size={14} />
              Plan a feature
            </Link>
            <Link className="btn" to="/workflows">
              Open Workflows
            </Link>
          </div>
        </Card>
      )}
      {status.state === "error" && !status.error?.isNoRun && status.error && (
        <ErrorPanel error={status.error} onRetry={status.reload} />
      )}
      {status.data && (
        <RunOverview
          status={status.data}
          events={events.data}
          control={control}
          plan={plan.data}
          health={health}
          attention={attention}
        />
      )}
    </section>
  );
}
