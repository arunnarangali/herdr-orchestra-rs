import { Link } from "react-router-dom";
import type { ApiError } from "../../api/errors";
import type { ReportResponse, StatusResponse } from "../../api/types";
import { Badge, Dot } from "../../ui/Badge";
import { Button, buttonClass, type Tone } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { useToast } from "../../ui/Toast";
import type { ProjectInfo } from "./backend";
import { CopyIcon } from "./CopyIcon";
import { runStateLabel } from "./format";

export const NOT_AVAILABLE = "Not available yet";

export interface RunInfo {
  status: StatusResponse | null;
  statusError: ApiError | null;
  report: ReportResponse | null;
}

/** How the project's own server stands: from the open project (desktop) or the connection. */
export interface ServerInfo {
  state: "running" | "starting" | "failed" | "stopped" | "unreachable";
  /** host:port, when known. */
  address: string | null;
  error?: string | null;
}

/** What the card offers when several projects can be open at once. */
export interface OpenControls {
  open: boolean;
  /** Where its run stands, from the background watcher (any open project, not only the shown one). */
  state: { text: string; tone: string } | null;
  onOpen: () => void;
  onShow: () => void;
  onClose: () => void;
}

export interface ProjectCardProps {
  project: ProjectInfo;
  active: boolean;
  busy: boolean;
  /** What the server says, for the project it serves (the active one). */
  run: RunInfo | null;
  server: ServerInfo;
  /** Items waiting for the developer in this project. */
  attention: number;
  onSwitch: () => void;
  onRemove: () => void;
  /** Set in the desktop app: open, show and close instead of switch. */
  controls?: OpenControls;
}

const TONE: Record<string, Tone> = { info: "cyan", success: "emerald", warning: "amber", danger: "coral", muted: "neutral" };
const runTone = (s: string): Tone => (s === "all_done" ? "emerald" : s === "running" ? "cyan" : s === "idle" ? "neutral" : s === "broken_branch" ? "coral" : "amber");

function runProblem(project: ProjectInfo, run: RunInfo): string {
  if (run.statusError?.isNoRun) return `No run yet${project.has_orchestrate ? " (the folder has an .orchestrate/)" : ""}.`;
  if (run.statusError?.isNetwork) return "The server is not answering.";
  if (run.statusError?.isUnauthorized) return "The server refused the token.";
  return "Loading…";
}

/** Merged and total milestones of the run on screen: from the report, else from the status. */
export function runProgress(run: RunInfo | null): { merged: number; total: number; percent: number } | null {
  if (!run) return null;
  let merged: number;
  let total: number;
  if (run.report) {
    ({ merged, total } = run.report);
  } else if (run.status) {
    total = run.status.progress.reduce((n, p) => n + p.count, 0);
    merged = run.status.progress.find((p) => p.state === "merged")?.count ?? 0;
  } else return null;
  return { merged, total, percent: total ? Math.round((merged / total) * 100) : 0 };
}

const SERVER: Record<ServerInfo["state"], { tone: Tone | "idle"; text: string }> = {
  running: { tone: "emerald", text: "Running" },
  starting: { tone: "cyan", text: "Starting…" },
  failed: { tone: "coral", text: "Did not start" },
  stopped: { tone: "idle", text: "Stopped" },
  unreachable: { tone: "coral", text: "Not answering" },
};

/** One project: where it lives, its server, its git branch, its run, and what can be done with it. */
export function ProjectCard({ project, active, busy, run, server, attention, onSwitch, onRemove, controls }: ProjectCardProps) {
  const toast = useToast();
  const missing = !project.exists;
  const status = run?.status ?? null;
  const progress = runProgress(run);
  const done = status?.run_state === "all_done";
  const srv = SERVER[server.state];

  const copyPath = async () => {
    try {
      await navigator.clipboard.writeText(project.path);
      toast(`Copied ${project.path}`, { tone: "cyan" });
    } catch {
      toast(`Copying is blocked here. The path is ${project.path}`, { tone: "amber", icon: "warning" });
    }
  };

  const headBadge = missing ? (
    <Badge tone="coral">Folder not found</Badge>
  ) : active ? (
    <Badge tone="emerald" pill>
      <Dot tone="emerald" />
      Active
    </Badge>
  ) : attention > 0 ? (
    <Badge tone="amber">Attention: {attention}</Badge>
  ) : controls?.open ? (
    <Badge pill>Open</Badge>
  ) : (
    <Badge pill>Idle</Badge>
  );

  const closeButton = controls?.open && (
    <Button disabled={busy} onClick={controls.onClose} aria-label={`Close ${project.name}`} title="Stops its server and closes its tab">
      <Icon name="x" size={14} />
      Close Project
    </Button>
  );

  let actions: React.ReactNode;
  if (active) {
    actions = (
      <>
        {!status ? (
          <Link className={buttonClass({ tone: "emerald", solid: true })} to="/chat">
            <Icon name="chat" size={14} />
            Plan a Feature
          </Link>
        ) : done ? (
          <>
            <Link className={buttonClass({ tone: "cyan" })} to="/reports">
              <Icon name="chart" size={14} />
              View Summary
            </Link>
            <Button tone="emerald" unavailable={NOT_AVAILABLE}>
              <Icon name="branch" size={14} />
              Finish &amp; Open PR
            </Button>
          </>
        ) : (
          <Link className={buttonClass({ tone: "emerald", solid: true })} to="/execution">
            <Icon name="external" size={14} />
            Open Run
          </Link>
        )}
        <Button tone="coral" unavailable={controls ? `${NOT_AVAILABLE}: Close Project stops its server` : NOT_AVAILABLE}>
          Stop Server
        </Button>
        {closeButton}
      </>
    );
  } else if (controls) {
    actions = (
      <>
        {controls.open ? (
          <Button tone="emerald" solid disabled={busy} onClick={controls.onShow}>
            <Icon name="external" size={14} />
            Show this project
          </Button>
        ) : (
          <Button tone="emerald" solid disabled={busy || missing} onClick={controls.onOpen}>
            <Icon name="folder" size={14} />
            Open project
          </Button>
        )}
        {closeButton}
      </>
    );
  } else {
    actions = (
      <Button tone="emerald" solid disabled={busy || missing} onClick={onSwitch}>
        <Icon name="sync" size={14} />
        Switch to this project
      </Button>
    );
  }

  return (
    <Card as="article" tone={active ? "emerald" : undefined} className="pcard" aria-label={project.name}>
      <header className="pcard__head">
        <span className="pcard__logo">
          <Icon name="cube" size={28} />
        </span>
        <div className="pcard__id">
          <h2 className="pcard__name">{project.name}</h2>
          <div className="pcard__pathlabel muted">
            Canonical Path
            <button type="button" className="pj-iconbtn" aria-label={`Copy path of ${project.name}`} onClick={() => void copyPath()}>
              <CopyIcon />
            </button>
          </div>
          <code className="pcard__path">{project.path}</code>
        </div>
        <div className="pcard__badges">
          {headBadge}
          {project.exists && !project.is_git && <Badge tone="amber">Not a git repository</Badge>}
        </div>
      </header>

      <div className="pcard__cols">
        <div className="pcard__server">
          <div className="pcard__k">
            <Dot tone={srv.tone} />
            Server Status
            {server.state !== "running" && <Badge tone={srv.tone === "idle" ? "neutral" : srv.tone}>{srv.text}</Badge>}
          </div>
          <div className="pcard__cmd">
            <span className="mono">{server.address ? `orch serve · ${server.address}` : `orch serve · ${server.state === "stopped" ? "not running" : "–"}`}</span>
            {server.state === "running" && <Badge tone="emerald">Token Active</Badge>}
            {server.state === "running" && attention > 0 && <Badge tone="amber">Attention: {attention}</Badge>}
          </div>
          {server.error && <p className="error-text pcard__small">{server.error}</p>}
        </div>
        <div className="pcard__git">
          <div className="pcard__gk">
            <Icon name="branch" size={14} />
            Git
          </div>
          <div className="pcard__gr">
            Branch: <span className="mono text-cyan">{project.branch ?? "–"}</span>
          </div>
          {status && (
            <div className="pcard__gr">
              Run branch: <span className="mono text-cyan">{status.branch}</span>
              {status.base && <Badge mono>Base: {status.base}</Badge>}
            </div>
          )}
          {status && (
            <div className="visually-hidden">
              {status.branch} from {status.base ?? "–"}
            </div>
          )}
          <div className="pcard__gr pcard__small">
            {project.has_orchestrate ? (
              <span className="text-emerald pcard__flag">
                <Icon name="check" size={14} />
                Prepared for runs (.orchestrate/)
              </span>
            ) : (
              <span className="muted">No .orchestrate/ yet</span>
            )}
          </div>
        </div>
      </div>

      <div className="pcard__run">
        <span className="pcard__runicon">
          <Icon name="activity" size={18} />
        </span>
        <div className="pcard__runbody">
          <div className="pcard__runlabel">
            <span className="muted">Active Run</span>
            {status && <Badge tone={runTone(status.run_state)}>{runStateLabel(status.run_state)}</Badge>}
            {!status && controls?.state && !run && <Badge tone={TONE[controls.state.tone] ?? "neutral"}>{controls.state.text}</Badge>}
          </div>
          {status ? (
            <div className="pcard__runtitle">{status.title}</div>
          ) : run ? (
            <div className={run.statusError?.isNetwork || run.statusError?.isUnauthorized ? "error-text pcard__small" : "muted pcard__small"}>{runProblem(project, run)}</div>
          ) : (
            <div className="muted pcard__small">
              {controls ? (controls.open ? "Show this project to see its run." : "Open this project to see its run.") : "Switch to this project to see its run."}
            </div>
          )}
          {progress && (
            <div className="pj-bar" role="progressbar" aria-label={`${project.name} run progress`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={progress.percent}>
              <i style={{ width: `${progress.percent}%` }} />
            </div>
          )}
        </div>
        {progress && (
          <div className="pcard__pct">
            <strong>{progress.percent}%</strong>
            <span className="muted pcard__small">
              {progress.merged} of {progress.total} merged
            </span>
          </div>
        )}
      </div>

      {missing && <p className="error-text">This folder is gone. Restore it, or remove it from the list.</p>}

      <div className="pcard__actions">
        {actions}
        <Button ghost disabled={busy} onClick={onRemove} aria-label={`Remove ${project.name} from the list`}>
          Remove from list
        </Button>
      </div>
      {missing && !active && <p className="muted pcard__small">{controls ? "Opening" : "Switching"} is disabled: the folder is not there.</p>}
    </Card>
  );
}
