import { useCallback, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { LiveStatus } from "../../components/LiveStatus";
import { NoticeList, NotificationOptIn, useNotices } from "../../components/Notices";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { LeadQueue } from "./LeadQueue";
import { RunControls } from "./RunControls";
import { TaskPanel } from "./TaskPanel";
import { TaskTable } from "./TaskTable";
import "./execution.css";

/** Watch the run live and act on it. Re-reads when the stream reports a change, and after every action. */
export function ExecutionScreen() {
  const tasks = useResource((api, signal) => api.tasks({ signal }), [], {
    refreshOn: ["task-state", "run-state"],
    isEmpty: (d) => d.tasks.length === 0,
  });
  const events = useResource((api, signal) => api.events({}, { signal }), [], {
    refreshOn: ["lead-event", "task-state"],
  });
  const [params, setParams] = useSearchParams();
  const openId = params.get("task");
  const [version, setVersion] = useState(0);
  const { notices, dismiss } = useNotices();

  const { reload: reloadTasks } = tasks;
  const { reload: reloadEvents } = events;
  const refresh = useCallback(() => {
    reloadTasks();
    reloadEvents();
    setVersion((v) => v + 1);
  }, [reloadTasks, reloadEvents]);

  const open = (id: string | null) =>
    setParams(
      (p) => {
        const n = new URLSearchParams(p);
        if (id) n.set("task", id);
        else n.delete("task");
        return n;
      },
      { replace: true },
    );

  return (
    <section className="screen execution" aria-labelledby="screen-title">
      <div className="screen-head">
        <h1 id="screen-title">Execution</h1>
        <LiveStatus />
        <NotificationOptIn />
      </div>
      <NoticeList notices={notices} dismiss={dismiss} />

      {tasks.state === "loading" && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {tasks.state === "error" && tasks.error?.isNoRun && (
        <div className="panel" role="status">
          <p className="panel-title">No run in this project yet</p>
          <p className="muted">Write .orchestrate/plan.md and run `orch sync`; this page fills in by itself.</p>
        </div>
      )}
      {tasks.state === "error" && tasks.error && !tasks.error.isNoRun && <ErrorPanel error={tasks.error} onRetry={tasks.reload} />}

      {(tasks.state === "ready" || tasks.state === "empty") && (
        <>
          <RunControls onChange={refresh} />
          {tasks.state === "empty" ? (
            <p className="muted">The run has no milestones.</p>
          ) : (
            <section className="panel" aria-label="Milestones">
              <h2 className="panel-title">Milestones</h2>
              <TaskTable tasks={tasks.data?.tasks ?? []} openId={openId} onOpen={open} />
            </section>
          )}
          {openId && (
            <TaskPanel
              key={openId}
              id={openId}
              task={tasks.data?.tasks.find((t) => t.id === openId)}
              version={version}
              onChange={refresh}
            />
          )}
          {events.data && <LeadQueue events={events.data} onChange={refresh} />}
          {events.state === "error" && events.error && <ErrorPanel error={events.error} onRetry={events.reload} />}
        </>
      )}
    </section>
  );
}
