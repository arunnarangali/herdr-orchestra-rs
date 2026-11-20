import "../components/components.css";
import { Link } from "react-router-dom";
import { useAttention } from "../components/Attention";
import { runStateLabel } from "../screens/projects/format";
import { Badge } from "../ui/Badge";
import type { Tone } from "../ui/Button";
import { useProjects, type OpenProject, type Runtime } from "./projects";

/** What to say next to a project's name: where it stands, in words. */
export function projectState(p: OpenProject, rt: Runtime | undefined): { text: string; tone: "info" | "success" | "warning" | "danger" | "muted" } {
  if (p.status === "starting") return { text: "Starting…", tone: "muted" };
  if (p.status === "failed") return { text: "Did not start", tone: "danger" };
  if (!rt || rt.hasRun === null) return { text: rt?.unreachable ? "Not answering" : "Checking…", tone: rt?.unreachable ? "danger" : "muted" };
  if (rt.unreachable) return { text: "Not answering", tone: "danger" };
  if (!rt.hasRun) return { text: "No run yet", tone: "muted" };
  if (!rt.runState) return { text: "Running", tone: "info" };
  const tone = rt.runState === "all_done" ? "success" : rt.runState === "stopped" || rt.runState === "broken_branch" ? "warning" : "info";
  return { text: runStateLabel(rt.runState), tone };
}

const BADGE_TONE: Record<"info" | "success" | "warning" | "danger" | "muted", Tone> = {
  info: "cyan",
  success: "emerald",
  warning: "amber",
  danger: "coral",
  muted: "neutral",
};

/**
 * The open projects, one tab each: its name, where its run stands and how many items wait for
 * the developer. The selected tab is the project every screen shows; the others keep running
 * and keep raising their attention items.
 */
export function ProjectBar() {
  const { multi, projects, selected, runtimes, select, close } = useProjects();
  const { counts } = useAttention();
  if (!multi || projects.length === 0) return null;
  return (
    <nav className="project-bar" aria-label="Open projects">
      <ul className="project-tabs">
        {projects.map((p) => {
          const st = projectState(p, runtimes[p.path]);
          const waiting = counts[p.path] ?? 0;
          const active = p.path === selected;
          return (
            <li key={p.path} className={active ? "project-tab project-tab--active" : "project-tab"} data-project={p.path}>
              <button
                type="button"
                className="project-tab-open"
                aria-current={active ? "true" : undefined}
                title={p.path}
                onClick={() => select(p.path)}
              >
                <span className="project-tab-name">{p.name}</span>
                <Badge tone={BADGE_TONE[st.tone]}>{st.text}</Badge>
                {waiting > 0 && (
                  <span className="project-tab__count" aria-label={`${waiting} ${waiting === 1 ? "item needs" : "items need"} attention`}>
                    {waiting}
                  </span>
                )}
              </button>
              <button type="button" className="project-tab-close" aria-label={`Close ${p.name}`} title={`Close ${p.name}: stops its server`} onClick={() => void close(p.path)}>
                ×
              </button>
            </li>
          );
        })}
        <li className="project-tab-add">
          <Link to="/projects" className="btn btn--sm" aria-label="Open or create a project">
            + Project
          </Link>
        </li>
      </ul>
    </nav>
  );
}

/** Shown in place of the screens while the selected project's server is starting or failed. */
export function ProjectProblem() {
  const { projects, selected, retry, close } = useProjects();
  const p = projects.find((x) => x.path === selected);
  if (!p || p.status === "ready") return null;
  if (p.status === "starting") {
    return (
      <p role="status" className="muted">
        Starting {p.name}…
      </p>
    );
  }
  return (
    <div className="panel panel--error project-problem" role="alert">
      <p className="panel-title">{p.name} did not start</p>
      <p>{p.error ?? "Its server did not start."}</p>
      <p className="muted">
        Only this project is affected; the others keep running. Fix the cause (for example the path of <code>orch</code> in Settings) and try again.
      </p>
      <div className="button-row">
        <button type="button" className="btn btn--cyan btn--solid" onClick={() => void retry(p.path)}>
          Try again
        </button>
        <button type="button" className="btn" onClick={() => void close(p.path)}>
          Close this project
        </button>
      </div>
    </div>
  );
}
