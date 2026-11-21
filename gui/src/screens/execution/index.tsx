import { useCallback, useState } from "react";
import { useSearchParams } from "react-router-dom";
import type { EventsResponse, PlanResponse, ReportResponse, StatusResponse, TasksResponse } from "../../api/types";
import { LiveStatus } from "../../components/LiveStatus";
import { NoticeList, NotificationOptIn, useNotices } from "../../components/Notices";
import { useRunControl } from "../../components/useRunControl";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { AgentCards, agentsOf } from "./AgentCards";
import { LeadQueue } from "./LeadQueue";
import { AUTOPILOT, Output } from "./Output";
import { RunProgress, monitorState } from "./RunProgress";
import { TaskPanel } from "./TaskPanel";
import { TaskTable } from "./TaskTable";
import "./execution.css";

/**
 * Follow the run live and steer it (prototype screen 03). Everything re-reads when the stream
 * reports a change; a re-read swaps the data in place and never moves keyboard focus.
 */
export function ExecutionScreen() {
  const { control, set } = useRunControl();
  const [params, setParams] = useSearchParams();
  const openId = params.get("task");
  const [version, setVersion] = useState(0);
  const [tab, setTab] = useState(AUTOPILOT);
  const { notices, dismiss } = useNotices();

  const tasks = useResource<TasksResponse>((api, signal) => api.tasks({ signal }), [], {
    refreshOn: ["task-state", "run-state"],
    isEmpty: (d) => d.tasks.length === 0,
  });
  const events = useResource<EventsResponse>((api, signal) => api.events({}, { signal }), [], {
    refreshOn: ["lead-event", "task-state"],
  });
  // Header facts are optional: the screen works without them, so their errors stay quiet.
  const status = useResource<StatusResponse>((api, signal) => api.status({ signal }), [], {
    refreshOn: ["task-state", "run-state"],
  });
  const plan = useResource<PlanResponse>((api, signal) => api.plan({ signal }), []);
  const report = useResource<ReportResponse>((api, signal) => api.report({ signal }), [], {
    refreshOn: ["task-state", "run-state"],
  });

  const { reload: reloadTasks } = tasks;
  const { reload: reloadEvents } = events;
  const { reload: reloadStatus } = status;
  const refresh = useCallback(() => {
    reloadTasks();
    reloadEvents();
    reloadStatus();
    setVersion((v) => v + 1);
  }, [reloadTasks, reloadEvents, reloadStatus]);

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

  const list = tasks.data?.tasks ?? [];
  const agents = agentsOf(status.data, list);
  const st = monitorState(status.data?.run_state ?? null, control);

  return (
    <section className="screen execution" aria-labelledby="screen-title">
      <div className="screen-head">
        <h1 id="screen-title" className="visually-hidden">
          Execution
        </h1>
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
        <div className="mon">
          <RunProgress
            status={status.data}
            tasks={list}
            budget={plan.data?.header.budget ?? null}
            timing={report.data}
            control={control}
            onControl={set}
            onChange={refresh}
          />
          <AgentCards agents={agents} onShowOutput={setTab} />
          <div className="mon__bottom">
            <Output agents={agents} active={tab} onSelect={setTab} />
            {events.data && <LeadQueue events={events.data} stateLabel={st.label} stateTone={st.tone} onOpen={open} onChange={refresh} />}
            {events.state === "error" && events.error && <ErrorPanel error={events.error} onRetry={events.reload} />}
          </div>
          {tasks.state === "empty" ? (
            <p className="muted">The run has no milestones.</p>
          ) : (
            <section className="panel milestones" aria-label="Milestones">
              <h2 className="panel-title">Milestones</h2>
              <TaskTable tasks={list} openId={openId} onOpen={open} />
            </section>
          )}
          {openId && (
            <TaskPanel key={openId} id={openId} task={list.find((t) => t.id === openId)} version={version} onChange={refresh} />
          )}
        </div>
      )}
    </section>
  );
}
