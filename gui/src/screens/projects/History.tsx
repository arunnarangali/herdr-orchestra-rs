import { useId, useState } from "react";
import { Link } from "react-router-dom";
import { Badge } from "../../ui/Badge";
import { Button, buttonClass } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { Table } from "../../ui/Table";
import type { ProjectInfo } from "./backend";
import { formatDuration, formatTime, runStateLabel } from "./format";
import { NOT_AVAILABLE, type RunInfo } from "./ProjectCard";

const SHOWN = 5;

interface Row {
  key: string;
  id: string;
  title: string;
  branch: string | null;
  milestones: { merged: number; total: number } | null;
  when: string;
  status: { text: string; tone: "emerald" | "cyan" | "amber" | "coral" | "neutral" };
  current: boolean;
  files?: string;
}

function rowsFor(project: ProjectInfo, run: RunInfo | null): Row[] {
  const rows: Row[] = [];
  const s = run?.status;
  if (s) {
    const r = run?.report;
    rows.push({
      key: "current",
      id: "current",
      title: s.title,
      branch: s.branch,
      milestones: r ? { merged: r.merged, total: r.total } : null,
      when: r?.elapsed_s != null ? formatDuration(r.elapsed_s) : "running",
      status: {
        text: runStateLabel(s.run_state),
        tone: s.run_state === "all_done" ? "emerald" : s.run_state === "running" ? "cyan" : s.run_state === "broken_branch" ? "coral" : "amber",
      },
      current: true,
    });
  }
  for (const a of project.archive) {
    rows.push({
      key: a.name,
      id: a.name,
      title: a.summary ?? "–",
      branch: null,
      milestones: null,
      when: formatTime(a.modified),
      status: { text: "Archived", tone: "neutral" },
      current: false,
      files: a.files.join(", ") || "empty",
    });
  }
  return rows;
}

/** The runs of one project: the current one (for the project on screen) and the archived ones. */
export function History({ projects, activePath, run }: { projects: ProjectInfo[]; activePath: string | null; run: RunInfo | null }) {
  const selectId = useId();
  const [chosen, setChosen] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const path = chosen && projects.some((p) => p.path === chosen) ? chosen : (projects.find((p) => p.path === activePath)?.path ?? projects[0]?.path);
  const project = projects.find((p) => p.path === path);
  if (!project) return null;
  const rows = rowsFor(project, project.path === activePath ? run : null);
  const shown = all ? rows : rows.slice(0, SHOWN);

  return (
    <Card as="section" className="hist" aria-labelledby="pj-hist-title">
      <div className="card__head hist__head">
        <span className="hist__icon">
          <Icon name="activity" size={24} />
        </span>
        <div className="hist__titles">
          <h2 className="card__title" id="pj-hist-title">
            Execution History &amp; Previous Runs
          </h2>
          <p className="muted hist__sub">Recent runs for {project.name}</p>
        </div>
        <label className="visually-hidden" htmlFor={selectId}>
          Project
        </label>
        <select
          id={selectId}
          className="hist__select"
          value={project.path}
          onChange={(e) => {
            setChosen(e.target.value);
            setAll(false);
          }}
        >
          {projects.map((p) => (
            <option key={p.path} value={p.path}>
              {p.name}
            </option>
          ))}
        </select>
        <Button size="sm" aria-expanded={all} disabled={rows.length <= SHOWN} onClick={() => setAll((a) => !a)}>
          {all ? "Show Less" : "View All"}
        </Button>
      </div>
      <Table caption={`Runs of ${project.name}`} className="hist__table">
        <thead>
          <tr>
            {["Run", "Plan Title", "Feature Branch", "Milestones", "Duration / Saved", "Status", "Actions"].map((c) => (
              <th key={c} scope="col">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {shown.length === 0 ? (
            <tr>
              <td colSpan={7} className="hist__empty">
                No earlier runs in .orchestrate/archive/.
              </td>
            </tr>
          ) : (
            shown.map((r) => (
              <tr key={r.key}>
                <td className="mono">{r.id}</td>
                <td>
                  {r.title}
                  {r.files && <div className="muted pcard__small">{r.files}</div>}
                </td>
                <td>{r.branch ? <span className="mono text-cyan">{r.branch}</span> : <span className="muted">–</span>}</td>
                <td className={r.milestones && r.milestones.merged < r.milestones.total ? "text-amber hist__nowrap" : "text-emerald hist__nowrap"}>
                  {r.milestones ? `${r.milestones.merged}/${r.milestones.total} Merged` : <span className="muted">–</span>}
                </td>
                <td className="hist__nowrap">{r.when}</td>
                <td>
                  <Badge tone={r.status.tone}>{r.status.text}</Badge>
                </td>
                <td>
                  {r.current ? (
                    <Link className={buttonClass({ tone: "cyan", size: "sm" })} to="/execution">
                      <Icon name="external" size={14} />
                      Open Run
                    </Link>
                  ) : (
                    <Button size="sm" tone="cyan" unavailable={`${NOT_AVAILABLE}: archived runs are not served by the API`}>
                      View Report
                    </Button>
                  )}
                </td>
              </tr>
            ))
          )}
        </tbody>
      </Table>
    </Card>
  );
}
