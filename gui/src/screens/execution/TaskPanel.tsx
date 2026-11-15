import type { TaskInfo } from "../../api/types";
import { TaskDetail } from "../../components/TaskDetail";
import { ErrorPanel } from "../../ui/ResourceView";
import { StatusBadge } from "../../ui/StatusBadge";
import { useResource } from "../../ui/useResource";
import { TaskActions } from "./TaskActions";

export interface TaskPanelProps {
  id: string;
  /** The row from the list, for the actions (the server re-validates every action). */
  task: TaskInfo | undefined;
  /** Bumps when an action succeeded, to re-read. */
  version: number;
  onChange: () => void;
}

/** One milestone opened: its actions and everything known about it. */
export function TaskPanel({ id, task, version, onChange }: TaskPanelProps) {
  const detail = useResource((api, signal) => api.task(id, { signal }), [id, version], {
    refreshOn: ["task-state"],
  });
  const logs = useResource((api, signal) => api.logs({ milestone: id, lines: 100 }, { signal }), [id, version], {
    refreshOn: ["log"],
    refreshDelayMs: 1000,
  });
  const current = detail.data?.task ?? task;

  return (
    <section className="panel" aria-label={`Milestone ${id}`}>
      <h2 className="panel-title">
        {id} {current?.title} {current && <StatusBadge state={current.state} />}
      </h2>
      {detail.state === "loading" && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {detail.state === "error" && detail.error && <ErrorPanel error={detail.error} onRetry={detail.reload} />}
      {current && <TaskActions task={current} onChange={onChange} />}
      {detail.data && <TaskDetail detail={detail.data} logs={logs.data ? logs.data.lines : logs.state === "error" ? [] : null} />}
    </section>
  );
}
