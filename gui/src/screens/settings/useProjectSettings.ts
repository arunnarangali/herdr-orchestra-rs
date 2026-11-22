import { useCallback, useEffect, useRef, useState } from "react";
import type { AgentCli, ProjectSettings, SaveProjectSettingsInput } from "../../api/types";
import { getBackend, type AppBackend } from "../projects/backend";

export interface ProjectSettingsState {
  backend: AppBackend | null;
  projectPath: string | null;
  clis: AgentCli[];
  saved: ProjectSettings | null;
  leadAgent: string;
  folder: string;
  loading: boolean;
  busy: boolean;
  isDirty: boolean;
  isSavedLeadMissing: boolean;
  savedLeadAgent: string | null;
  folderError: string | null;
  saveError: string | null;
  saveSuccess: string | null;
  loadWarning: string | null;
  loadError: string | null;
  setLeadAgent: (v: string) => void;
  setFolder: (v: string) => void;
  save: () => Promise<boolean>;
  reload: () => Promise<void>;
}

export interface UseProjectSettingsOptions {
  onFolderChanged?: (newFolder: string) => void;
}

export function useProjectSettings(
  projectPath: string | null,
  options?: UseProjectSettingsOptions,
): ProjectSettingsState {
  // once per screen: in the desktop app `getBackend()` is a new object on every call, and
  // `load` depends on it, so reading it each render would reload (and reset the form) forever
  const [backend] = useState(getBackend);
  const [clis, setClis] = useState<AgentCli[]>([]);
  const [saved, setSaved] = useState<ProjectSettings | null>(null);
  const [leadAgent, setLeadAgent] = useState("");
  const [folder, setFolder] = useState("");
  const [loading, setLoading] = useState(Boolean(backend && projectPath));
  const [busy, setBusy] = useState(false);
  const [folderError, setFolderError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState<string | null>(null);
  const [loadWarning, setLoadWarning] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [effectivePath, setEffectivePath] = useState(projectPath);
  const [lastExternalPath, setLastExternalPath] = useState(projectPath);
  const lastSavedFolder = useRef<string | null>(null);

  if (projectPath !== lastExternalPath) {
    setLastExternalPath(projectPath);
    if (projectPath === lastSavedFolder.current) {
      lastSavedFolder.current = null;
    } else if (projectPath === null && lastSavedFolder.current !== null) {
      // transient reconnecting state right after save
    } else if (projectPath !== effectivePath) {
      setEffectivePath(projectPath);
      setSaved(null);
      setLoading(Boolean(backend && projectPath));
      setSaveSuccess(null);
      setSaveError(null);
      setFolderError(null);
      setLoadError(null);
    }
  }

  const load = useCallback(async () => {
    if (!effectivePath) {
      setLoading(false);
      setSaved(null);
      setLeadAgent("");
      setFolder("");
      setLoadError(null);
      return;
    }
    if (!backend) {
      setLoading(false);
      setClis([]);
      setSaved({ leadAgent: null, folder: effectivePath, warning: null });
      setLeadAgent("");
      setFolder(effectivePath);
      setLoadError(null);
      return;
    }
    setLoading(true);
    setSaveError(null);
    setFolderError(null);
    setLoadError(null);
    try {
      const [detected, settings] = await Promise.all([
        backend.detectAgentClis ? backend.detectAgentClis() : Promise.resolve([]),
        backend.getProjectSettings
          ? backend.getProjectSettings(effectivePath)
          : Promise.resolve({ leadAgent: null, folder: effectivePath, warning: null }),
      ]);
      setClis(detected);
      setSaved(settings);
      setLeadAgent(settings.leadAgent ?? "");
      setFolder(settings.folder || effectivePath);
      setLoadWarning(settings.warning);
      setLoadError(null);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      setLoadError(msg);
      setSaveError(msg);
    } finally {
      setLoading(false);
    }
  }, [backend, effectivePath]);

  useEffect(() => {
    void load();
  }, [load]);

  const isDirty = Boolean(
    saved &&
      (leadAgent !== (saved.leadAgent ?? "") ||
        folder !== (saved.folder || (effectivePath ?? ""))),
  );

  const savedLeadAgent = saved?.leadAgent ?? null;
  const isSavedLeadMissing = Boolean(
    savedLeadAgent && !clis.some((c) => c.kind === savedLeadAgent && c.installed),
  );

  const save = useCallback(async (): Promise<boolean> => {
    if (!backend || !effectivePath || !isDirty || busy) return false;
    setBusy(true);
    setSaveError(null);
    setFolderError(null);
    setSaveSuccess(null);
    try {
      const input: SaveProjectSettingsInput = {
        leadAgent: leadAgent.trim() || null,
        folder: folder.trim(),
      };
      const updated = backend.saveProjectSettings
        ? await backend.saveProjectSettings(effectivePath, input)
        : { leadAgent: input.leadAgent, folder: input.folder, warning: null };
      const oldPath = effectivePath;
      setSaved(updated);
      setLeadAgent(updated.leadAgent ?? "");
      setFolder(updated.folder);
      setEffectivePath(updated.folder);
      setSaveSuccess("Project settings saved.");
      if (updated.folder !== oldPath) {
        lastSavedFolder.current = updated.folder;
        options?.onFolderChanged?.(updated.folder);
      }
      return true;
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      if (/git repository|folder|cannot open/i.test(msg)) {
        setFolderError(msg);
      }
      setSaveError(msg);
      return false;
    } finally {
      setBusy(false);
    }
  }, [backend, effectivePath, isDirty, busy, leadAgent, folder, options]);

  return {
    backend,
    projectPath: effectivePath,
    clis,
    saved,
    leadAgent,
    folder,
    loading,
    busy,
    isDirty,
    isSavedLeadMissing,
    savedLeadAgent,
    folderError,
    saveError,
    saveSuccess,
    loadWarning,
    loadError,
    setLeadAgent: (v) => {
      setLeadAgent(v);
      setSaveSuccess(null);
    },
    setFolder: (v) => {
      setFolder(v);
      setFolderError(null);
      setSaveSuccess(null);
    },
    save,
    reload: load,
  };
}
