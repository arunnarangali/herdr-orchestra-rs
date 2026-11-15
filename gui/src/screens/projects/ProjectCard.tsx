import type { ApiError } from "../../api/errors";
import type { ReportResponse, StatusResponse } from "../../api/types";
import { formatDuration, formatTime, runStateLabel } from "./format";
import type { ProjectInfo } from "./backend";

export interface RunInfo {
  status: StatusResponse | null;
  statusError: ApiError | null;
  report: ReportResponse | null;
}

export interface ProjectCardProps {
  project: ProjectInfo;
  active: boolean;
  busy: boolean;
  /** What the server says, for the project it serves (the active one). */
  run: RunInfo | null;
  onSwitch: () => void;
  onRemove: () => void;
}

function runLine(project: ProjectInfo, run: RunInfo | null) {
  if (!run) {
    return <span className="muted">Switch to this project to see its run.</span>;
  }
  if (run.status) {
    return <span className="badge badge--info">{runStateLabel(run.status.run_state)}</span>;
  }
  if (run.statusError?.isNoRun) return <span className="muted">No run yet{project.has_orchestrate ? " (the folder has an .orchestrate/)" : ""}.</span>;
  if (run.statusError?.isNetwork) return <span className="error-text">The server is not answering.</span>;
  if (run.statusError?.isUnauthorized) return <span className="error-text">The server refused the token.</span>;
  return <span className="muted">Loading…</span>;
}

/** One project: path, branch, base, run state and the history of earlier runs. */
export function ProjectCard({ project, active, busy, run, onSwitch, onRemove }: ProjectCardProps) {
  const missing = !project.exists;
  return (
    <article className={active ? "panel project project--active" : "panel project"} aria-label={project.name}>
      <header className="project-head">
        <h2 className="panel-title">{project.name}</h2>
        {active && <span className="badge badge--info">Active</span>}
        {missing && <span className="badge badge--danger">Folder not found</span>}
        {project.exists && !project.is_git && <span className="badge badge--warning">Not a git repository</span>}
      </header>
      <dl className="pj-facts">
        <div>
          <dt>Path</dt>
          <dd>
            <code>{project.path}</code>
          </dd>
        </div>
        <div>
          <dt>Branch</dt>
          <dd>{project.branch ?? "–"}</dd>
        </div>
        <div>
          <dt>Run branch / base</dt>
          <dd>{run?.status ? `${run.status.branch} from ${run.status.base ?? "–"}` : "–"}</dd>
        </div>
        <div>
          <dt>Run state</dt>
          <dd>{runLine(project, run)}</dd>
        </div>
        {run?.report && (
          <div>
            <dt>Report</dt>
            <dd>
              {run.report.merged} of {run.report.total} merged
              {run.report.elapsed_s !== null && `, ${formatDuration(run.report.elapsed_s)}`}
            </dd>
          </div>
        )}
      </dl>
      {missing && <p className="error-text">This folder is gone. Restore it, or remove it from the list.</p>}

      <h3 className="project-sub">Earlier runs</h3>
      {project.archive.length === 0 ? (
        <p className="muted">No earlier runs in .orchestrate/archive/.</p>
      ) : (
        <ul className="pj-list">
          {project.archive.map((a) => (
            <li key={a.name}>
              <strong>{a.name}</strong> <span className="muted">{formatTime(a.modified)}</span>
              {a.summary && <div>{a.summary}</div>}
              <div className="muted">{a.files.join(", ") || "empty"}</div>
            </li>
          ))}
        </ul>
      )}

      <div className="pj-actions">
        {!active && (
          <button type="button" className="button button--primary" disabled={busy || missing} onClick={onSwitch}>
            Switch to this project
          </button>
        )}
        <button type="button" className="button" disabled={busy} onClick={onRemove} aria-label={`Remove ${project.name} from the list`}>
          Remove from list
        </button>
      </div>
      {missing && !active && <p className="muted">Switching is disabled: the folder is not there.</p>}
    </article>
  );
}
