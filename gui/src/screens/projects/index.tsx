import { useCallback, useEffect, useMemo, useState } from "react";
import { useConnection } from "../../app/connection";
import { projectState } from "../../app/ProjectBar";
import { useProjects } from "../../app/projects";
import { useAttention } from "../../components/Attention";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Icon } from "../../ui/Icon";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { getBackend, type ProjectInfo, type ProjectList } from "./backend";
import { History } from "./History";
import { ProjectCard, type ServerInfo } from "./ProjectCard";
import "./projects.css";

/** The projects the app remembers: add, remove and switch (which restarts the server). */
export function ProjectsScreen() {
  const backend = useMemo(() => getBackend(), []);
  const { connect, health, client, credentials, kind } = useConnection();
  const { counts } = useAttention();
  const pj = useProjects();
  const [list, setList] = useState<ProjectList | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [message, setMessage] = useState<{ kind: "ok" | "error"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<ProjectInfo | null>(null);

  const load = useCallback(() => {
    if (!backend) return;
    backend.listProjects().then(
      (l) => {
        setList(l);
        setLoadError(null);
      },
      (e: unknown) => setLoadError(e instanceof Error ? e.message : String(e)),
    );
  }, [backend]);
  useEffect(load, [load]);

  // What the server says about the project it serves: base, run state, report.
  const status = useResource((api, signal) => api.status({ signal }), [client]);
  const report = useResource((api, signal) => api.report({ signal }), [client]);

  const act = async (what: () => Promise<void>) => {
    setBusy(true);
    setMessage(null);
    pj.clearNotice();
    try {
      await what();
    } catch (e) {
      setMessage({ kind: "error", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  const add = () =>
    act(async () => {
      const folder = await backend?.chooseFolder();
      if (!folder || !backend) return;
      if (pj.multi) {
        await pj.openFolder(folder);
        setList(await backend.listProjects());
        return;
      }
      setList(await backend.addProject(folder));
      setMessage({ kind: "ok", text: `Added ${folder}.` });
    });
  // A new project: the app makes the git repository and prepares the folder, then opens it.
  const create = () =>
    act(async () => {
      const folder = await backend?.chooseFolder();
      if (!folder || !backend) return;
      await pj.create(folder);
      setList(await backend.listProjects());
    });
  const openIt = (p: ProjectInfo) =>
    act(async () => {
      await pj.openListed(p.path);
      if (backend) setList(await backend.listProjects());
    });
  const closeIt = (p: ProjectInfo) =>
    act(async () => {
      await pj.close(p.path);
      if (backend) setList(await backend.listProjects());
    });
  const remove = (p: ProjectInfo) =>
    act(async () => {
      if (!backend) return;
      setList(await backend.removeProject(p.path));
      setMessage({ kind: "ok", text: `Removed ${p.path} from the list. Its files were not touched.` });
    });
  const switchTo = (p: ProjectInfo) =>
    act(async () => {
      if (!backend) return;
      connect(await backend.switchProject(p.path));
      setList(await backend.listProjects());
      setMessage({ kind: "ok", text: `Switched to ${p.name}: the server was restarted for it.` });
    });

  const served = health?.project ?? null;
  const activePath = pj.multi
    ? pj.selected
    : (list?.active ?? (list?.projects.find((p) => p.path === served)?.path ?? null));

  const run = { status: status.data, statusError: status.error, report: report.data };
  const address = (url: string | null | undefined) => (url ? url.replace(/^https?:\/\//, "").replace(/\/$/, "") : null);
  // With one server its items carry no project; they belong to the project on screen.
  const attentionOf = (path: string) => (pj.multi ? (counts[path] ?? 0) : path === activePath ? (counts[""] ?? 0) : 0);
  /** Where a project's own server stands. */
  const serverOf = (path: string): ServerInfo => {
    if (pj.multi) {
      const o = pj.projects.find((x) => x.path === path);
      if (!o) return { state: "stopped", address: null };
      if (o.status === "starting") return { state: "starting", address: null };
      if (o.status === "failed") return { state: "failed", address: null, error: o.error };
      return { state: pj.runtimes[path]?.unreachable ? "unreachable" : "running", address: address(o.credentials?.url) };
    }
    if (path !== activePath) return { state: "stopped", address: null };
    if (kind === "no-server") return { state: "unreachable", address: address(credentials?.url) };
    // The server answered a read: it runs, whatever the health check still says.
    if (kind === "checking" && !status.data) return { state: "starting", address: address(credentials?.url) };
    return { state: credentials ? "running" : "stopped", address: address(credentials?.url) };
  };

  return (
    <section className="screen pj" aria-labelledby="screen-title">
      <header className="pg-head">
        <span className="pg-head__icon">
          <Icon name="folder" />
        </span>
        <div className="pg-head__titles">
          <h1 id="screen-title" className="pg-head__title">
            Projects &amp; Workspaces
          </h1>
          <p className="pg-head__sub">Manage your projects, repositories and running workspaces</p>
        </div>
        <div className="pg-head__side">
          <Button disabled={busy} unavailable={backend ? undefined : "Available in the desktop app"} onClick={() => void add()}>
            <Icon name="folder" size={14} />+ Add Existing Folder
          </Button>
          <Button
            tone="emerald"
            solid
            disabled={busy}
            unavailable={backend && pj.multi ? undefined : "Available in the desktop app"}
            onClick={() => void create()}
          >
            <Icon name="plus" size={14} />
            Create New Project
          </Button>
        </div>
      </header>

      {(() => {
        const shown = message ?? pj.notice;
        return (
          shown && (
            <p className={shown.kind === "ok" ? "pj-msg tone-emerald" : "pj-msg tone-coral"} role={shown.kind === "ok" ? "status" : "alert"}>
              {shown.text}
            </p>
          )
        );
      })()}

      {!backend && (
        <Card className="pj-note" role="status">
          <p className="panel-title">Connected to one server</p>
          <p>
            {served ? (
              <>
                This page is connected to the project at <code>{served}</code>.
              </>
            ) : (
              "This page is not connected to a project yet."
            )}
          </p>
          <p className="muted">
            The project list is kept by the desktop app. In a browser, start <code>orch serve</code> in another
            project and connect to it from the frame's connection panel.
          </p>
        </Card>
      )}

      {backend && loadError && (
        <p className="pj-msg tone-coral" role="alert">
          Cannot read the project list: {loadError}
        </p>
      )}
      {backend && !list && !loadError && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {list?.warning && (
        <p className="pj-msg tone-coral" role="alert">
          {list.warning}
        </p>
      )}
      {list && list.projects.length === 0 && (
        <Card className="pj-note" role="status">
          <Icon name="folder" size={28} />
          <p className="panel-title">No projects yet</p>
          <p className="muted">
            Choose “Add Existing Folder” and pick a folder that contains a git repository
            {pj.multi ? ", or “Create New Project” to start one in an empty or new folder" : ""}.
          </p>
        </Card>
      )}

      {list && list.projects.length > 0 && (
        <ul className="pcards" aria-label="Projects">
          {list.projects.map((p) => (
            <li key={p.path}>
              <ProjectCard
                project={p}
                active={p.path === activePath}
                busy={busy}
                run={p.path === activePath ? run : null}
                server={serverOf(p.path)}
                attention={attentionOf(p.path)}
                controls={
                  pj.multi
                    ? (() => {
                        const o = pj.projects.find((x) => x.path === p.path);
                        return {
                          open: o !== undefined,
                          state: o ? projectState(o, pj.runtimes[o.path]) : null,
                          onOpen: () => void openIt(p),
                          onShow: () => pj.select(p.path),
                          onClose: () => void closeIt(p),
                        };
                      })()
                    : undefined
                }
                onSwitch={() => void switchTo(p)}
                onRemove={() => setRemoving(p)}
              />
            </li>
          ))}
        </ul>
      )}

      {list && list.projects.length > 0 && <History projects={list.projects} activePath={activePath} run={run} />}

      {removing && (
        <ConfirmDialog
          open
          title={`Remove ${removing.name} from the list?`}
          confirmLabel="Remove from list"
          destructive
          onConfirm={() => {
            const p = removing;
            setRemoving(null);
            void remove(p);
          }}
          onCancel={() => setRemoving(null)}
        >
          Only the app's list changes. The folder <code>{removing.path}</code> and its files are not deleted.
        </ConfirmDialog>
      )}
      {!backend && status.state === "error" && status.error && !status.error.isNoRun && (
        <ErrorPanel error={status.error} onRetry={status.reload} />
      )}
    </section>
  );
}
