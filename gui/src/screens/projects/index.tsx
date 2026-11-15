import { useCallback, useEffect, useMemo, useState } from "react";
import { useConnection } from "../../app/connection";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { getBackend, type ProjectInfo, type ProjectList } from "./backend";
import { ProjectCard } from "./ProjectCard";
import "./projects.css";

/** The projects the app remembers: add, remove and switch (which restarts the server). */
export function ProjectsScreen() {
  const backend = useMemo(() => getBackend(), []);
  const { connect, health, client } = useConnection();
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
      setList(await backend.addProject(folder));
      setMessage({ kind: "ok", text: `Added ${folder}.` });
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
  const activePath = list?.active ?? (list?.projects.find((p) => p.path === served)?.path ?? null);

  return (
    <section className="screen projects" aria-labelledby="screen-title">
      <div className="pj-head">
        <h1 id="screen-title">Projects</h1>
        {backend && (
          <button type="button" className="button button--primary" onClick={() => void add()} disabled={busy}>
            Add project…
          </button>
        )}
      </div>

      {message && (
        <p className={message.kind === "ok" ? "pj-msg pj-msg--ok" : "pj-msg pj-msg--error"} role={message.kind === "ok" ? "status" : "alert"}>
          {message.text}
        </p>
      )}

      {!backend && (
        <div className="panel" role="status">
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
        </div>
      )}

      {backend && loadError && (
        <p className="pj-msg pj-msg--error" role="alert">
          Cannot read the project list: {loadError}
        </p>
      )}
      {backend && !list && !loadError && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {list?.warning && (
        <p className="pj-msg pj-msg--error" role="alert">
          {list.warning}
        </p>
      )}
      {list && list.projects.length === 0 && (
        <div className="panel" role="status">
          <p className="panel-title">No projects yet</p>
          <p className="muted">Choose “Add project…” and pick a folder that contains a git repository.</p>
        </div>
      )}

      {list && list.projects.length > 0 && (
        <ul className="project-list" aria-label="Projects">
          {list.projects.map((p) => (
            <li key={p.path}>
              <ProjectCard
                project={p}
                active={p.path === activePath}
                busy={busy}
                run={p.path === activePath ? { status: status.data, statusError: status.error, report: report.data } : null}
                onSwitch={() => void switchTo(p)}
                onRemove={() => setRemoving(p)}
              />
            </li>
          ))}
        </ul>
      )}

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
