import { useCallback, useEffect, useMemo, useState } from "react";
import type { AgentCli } from "../../api/types";
import { useConnection } from "../../app/connection";
import { projectState } from "../../app/ProjectBar";
import { useProjects } from "../../app/projects";
import { useAttention } from "../../components/Attention";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Icon } from "../../ui/Icon";
import { Modal } from "../../ui/Modal";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { getBackend, type ProjectInfo, type ProjectList } from "./backend";
import { History } from "./History";
import { ProjectCard, type ServerInfo } from "./ProjectCard";
import "./projects.css";

function nameOf(path: string): string {
  return path.replace(/\/+$/, "").split("/").pop() || path;
}

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

  const [clis, setClis] = useState<AgentCli[]>([]);
  const [addingFolder, setAddingFolder] = useState<string | null>(null);
  const [selectedLead, setSelectedLead] = useState("");
  const [addError, setAddError] = useState<string | null>(null);
  const [addBusy, setAddBusy] = useState(false);

  const [creatingFolder, setCreatingFolder] = useState<string | null>(null);
  const [createLead, setCreateLead] = useState("");
  const [createError, setCreateError] = useState<string | null>(null);
  const [createBusy, setCreateBusy] = useState(false);

  useEffect(() => {
    if (!backend?.detectAgentClis) return;
    backend.detectAgentClis().then(setClis, () => {});
  }, [backend]);

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

  // The project is already added; saving its Lead can still fail. Say so plainly and where to fix it.
  const saveLeadAfter = async (
    folder: string,
    lead: string,
    okText: string | null,
  ): Promise<{ kind: "ok" | "error"; text: string } | null> => {
    if (lead && backend?.saveProjectSettings) {
      try {
        await backend.saveProjectSettings(folder, { leadAgent: lead, folder });
      } catch (e: unknown) {
        const why = e instanceof Error ? e.message : String(e);
        return {
          kind: "error",
          text: `Added ${folder}, but its default Lead agent was not saved: ${why}. Choose it again in Settings.`,
        };
      }
    }
    return okText ? { kind: "ok", text: okText } : null;
  };

  const add = async () => {
    setMessage(null);
    const folder = await backend?.chooseFolder();
    if (!folder || !backend) return;
    setAddingFolder(folder);
    setSelectedLead("");
    setAddError(null);
  };

  const submitAdd = async () => {
    if (!backend || !addingFolder || addBusy) return;
    setAddBusy(true);
    setAddError(null);
    try {
      if (pj.multi) {
        await pj.openFolder(addingFolder);
        setList(await backend.listProjects());
      } else {
        setList(await backend.addProject(addingFolder));
      }
    } catch (e: unknown) {
      // Nothing was added: the dialog stays open so the same choice can be retried.
      setAddError(e instanceof Error ? e.message : String(e));
      setAddBusy(false);
      return;
    }
    // The project exists from here on, so the dialog closes whatever happens to the lead.
    const folder = addingFolder;
    setAddingFolder(null);
    setMessage(await saveLeadAfter(folder, selectedLead, `Added ${folder}.`));
    setAddBusy(false);
  };

  // A new project: the app makes the git repository and prepares the folder, then opens it.
  const create = async () => {
    setMessage(null);
    const folder = await backend?.chooseFolder();
    if (!folder || !backend) return;
    setCreatingFolder(folder);
    setCreateLead("");
    setCreateError(null);
  };

  const submitCreate = async () => {
    if (!backend || !creatingFolder || createBusy) return;
    setCreateBusy(true);
    setCreateError(null);
    try {
      await pj.create(creatingFolder);
    } catch (e: unknown) {
      setCreateError(e instanceof Error ? e.message : String(e));
      setCreateBusy(false);
      return;
    }
    const folder = creatingFolder;
    setCreatingFolder(null);
    const result = await saveLeadAfter(folder, createLead, null);
    if (result) setMessage(result);
    try {
      setList(await backend.listProjects());
    } catch {
      // the list refreshes on the next visit
    }
    setCreateBusy(false);
  };
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

      {addingFolder && (
        <Modal
          open
          title={`Add Project: ${nameOf(addingFolder)}`}
          onClose={() => {
            if (!addBusy) setAddingFolder(null);
          }}
        >
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void submitAdd();
            }}
          >
            <div className="modal-body">
              <div className="field">
                <label htmlFor="add-project-path">Folder Path</label>
                <input
                  id="add-project-path"
                  className="mono"
                  value={addingFolder}
                  readOnly
                />
              </div>

              <div className="field" style={{ marginTop: 12 }}>
                <label htmlFor="add-lead-agent">Default Lead Agent</label>
                <select
                  id="add-lead-agent"
                  aria-label="Default Lead Agent"
                  value={selectedLead}
                  disabled={addBusy}
                  onChange={(e) => setSelectedLead(e.target.value)}
                >
                  <option value="">None (no default lead agent)</option>
                  {clis.map((c) => (
                    <option
                      key={c.kind}
                      value={c.kind}
                      disabled={!c.installed || !c.canLead}
                    >
                      {c.displayName} ({c.installed ? "Installed" : "Not installed"})
                    </option>
                  ))}
                </select>
                <p className="muted" style={{ fontSize: "0.85em", marginTop: 4 }}>
                  The agent CLI used to plan and run milestones in this project.
                </p>
              </div>

              {addError && (
                <p className="field-error tone-coral" role="alert" style={{ marginTop: 12 }}>
                  {addError}
                </p>
              )}
            </div>

            <div className="modal-actions" style={{ marginTop: 16, display: "flex", justifyContent: "flex-end", gap: 8 }}>
              <Button
                type="button"
                disabled={addBusy}
                onClick={() => setAddingFolder(null)}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                tone="emerald"
                solid
                disabled={addBusy}
              >
                Add Project
              </Button>
            </div>
          </form>
        </Modal>
      )}

      {creatingFolder && (
        <Modal
          open
          title={`Create New Project: ${nameOf(creatingFolder)}`}
          onClose={() => {
            if (!createBusy) setCreatingFolder(null);
          }}
        >
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void submitCreate();
            }}
          >
            <div className="modal-body">
              <div className="field">
                <label htmlFor="create-project-path">Folder Path</label>
                <input
                  id="create-project-path"
                  className="mono"
                  value={creatingFolder}
                  readOnly
                />
              </div>

              <div className="field" style={{ marginTop: 12 }}>
                <label htmlFor="create-lead-agent">Default Lead Agent</label>
                <select
                  id="create-lead-agent"
                  aria-label="Default Lead Agent"
                  value={createLead}
                  disabled={createBusy}
                  onChange={(e) => setCreateLead(e.target.value)}
                >
                  <option value="">None (no default lead agent)</option>
                  {clis.map((c) => (
                    <option
                      key={c.kind}
                      value={c.kind}
                      disabled={!c.installed || !c.canLead}
                    >
                      {c.displayName} ({c.installed ? "Installed" : "Not installed"})
                    </option>
                  ))}
                </select>
                <p className="muted" style={{ fontSize: "0.85em", marginTop: 4 }}>
                  The agent CLI used to plan and run milestones in this project.
                </p>
              </div>

              {createError && (
                <p className="field-error tone-coral" role="alert" style={{ marginTop: 12 }}>
                  {createError}
                </p>
              )}
            </div>

            <div className="modal-actions" style={{ marginTop: 16, display: "flex", justifyContent: "flex-end", gap: 8 }}>
              <Button
                type="button"
                disabled={createBusy}
                onClick={() => setCreatingFolder(null)}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                tone="emerald"
                solid
                disabled={createBusy}
              >
                Create Project
              </Button>
            </div>
          </form>
        </Modal>
      )}
    </section>
  );
}
