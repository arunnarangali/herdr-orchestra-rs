import type { TaskInfo } from "../../api/types";
import { needsHuman } from "../../components/needsHuman";
import { StatusBadge } from "../../ui/StatusBadge";

export interface TaskTableProps {
  tasks: TaskInfo[];
  openId: string | null;
  onOpen: (id: string | null) => void;
}

/** Every milestone with its state, worker, model, attempts and rounds. A milestone waiting for the developer says so. */
export function TaskTable({ tasks, openId, onOpen }: TaskTableProps) {
  return (
    <div className="table-wrap">
      <table className="table exec-table">
        <caption className="visually-hidden">Milestones</caption>
        <thead>
          <tr>
            <th scope="col">Milestone</th>
            <th scope="col">State</th>
            <th scope="col">Worker</th>
            <th scope="col">Model</th>
            <th scope="col" title="attempts">Tries</th>
            <th scope="col" title="fix, review and conflict rounds">Rounds (fix/review/conflict)</th>
          </tr>
        </thead>
        <tbody>
          {tasks.map((t) => {
            const need = needsHuman(t.state, t.reason);
            const open = t.id === openId;
            return (
              <tr key={t.id} className={`${need ? "exec-row exec-row--needs" : "exec-row"}${open ? " exec-row--open" : ""}`}>
                <th scope="row">
                  <button
                    type="button"
                    className="link-button"
                    aria-expanded={open}
                    onClick={() => onOpen(open ? null : t.id)}
                  >
                    <strong>{t.id}</strong> {t.title}
                  </button>
                  {need && (
                    <div className="needs">
                      <span className="badge badge--warning">{need.label}</span> <span>{need.text}</span>
                    </div>
                  )}
                </th>
                <td>
                  <StatusBadge state={t.state} />
                  {t.ready && <span className="muted"> ready</span>}
                  {t.escalated && <span className="muted"> escalated</span>}
                </td>
                <td>{t.worker ?? t.last_worker ?? "–"}</td>
                <td>{t.model ?? "–"}</td>
                <td>{t.attempt}</td>
                <td>
                  {t.fix_rounds}/{t.review_rounds}/{t.conflict_rounds}
                  {t.lost > 0 && <span className="muted"> · {t.lost} lost</span>}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
