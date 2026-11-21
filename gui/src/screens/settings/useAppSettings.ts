import { useEffect, useState } from "react";
import { useTheme, type ThemePreference } from "../../app/theme";
import { getBackend, type AppBackend, type AppSettings } from "../projects/backend";

export type Outcome = { kind: "ok" | "error"; text: string } | null;

const DEFAULTS: AppSettings = { theme: "system", notifications: false, orch_path: "" };

export interface AppSettingsState {
  /** The shell's storage; null in a browser, where only the theme can be kept (by this browser). */
  backend: AppBackend | null;
  /** What is stored now, with the theme that is on screen (the two are kept in step). */
  saved: AppSettings;
  loaded: boolean;
  /** A problem reading the stored settings (they were corrupt and were kept aside). */
  loadWarning: string | null;
  /** Where orch was found now, or null when it was not. */
  detected: string | null;
  busy: boolean;
  /** What the last save did: shown next to the settings, success or the reason it failed. */
  outcome: Outcome;
  /** The orch path field, as typed (saved with `saveOrchPath`). */
  orch: string;
  setOrch: (v: string) => void;
  changeTheme: (v: ThemePreference) => Promise<void>;
  changeNotifications: (on: boolean) => Promise<void>;
  saveOrchPath: () => Promise<void>;
}

/**
 * The application's own settings. In the desktop app they are saved by the shell and survive a
 * restart; a save that fails says why and leaves the old value. In a browser only the theme is
 * offered, and this browser keeps it.
 */
export function useAppSettings(): AppSettingsState {
  const [backend] = useState(getBackend);
  const { preference, setPreference } = useTheme();
  const [saved, setSaved] = useState<AppSettings>({ ...DEFAULTS, theme: preference });
  const [orch, setOrch] = useState("");
  const [detected, setDetected] = useState<string | null>(null);
  const [loadWarning, setLoadWarning] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(!backend);
  const [outcome, setOutcome] = useState<Outcome>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!backend) return;
    let cancelled = false;
    backend.getSettings().then(
      ({ value, warning }) => {
        if (cancelled) return;
        setSaved(value);
        setOrch(value.orch_path);
        setLoadWarning(warning);
        setLoaded(true);
        // The theme on screen (also set by the top bar toggle) is the one that counts: the stored
        // copy is brought in step below, never applied over it.
      },
      (e: unknown) => {
        if (cancelled) return;
        setOutcome({ kind: "error", text: `Cannot read the saved settings: ${e instanceof Error ? e.message : String(e)}` });
        setLoaded(true);
      },
    );
    backend.findOrch().then(
      (p) => !cancelled && setDetected(p),
      () => !cancelled && setDetected(null),
    );
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- read once when the screen opens
  }, []);

  // Keep the shell's copy of the theme in step with the theme on screen (the top bar toggle
  // writes only the browser's storage). A failure says so; nothing on screen changes.
  useEffect(() => {
    if (!backend || !loaded || busy || saved.theme === preference) return;
    let cancelled = false;
    backend.saveSettings({ ...saved, theme: preference }).then(
      (stored) => !cancelled && setSaved(stored),
      (e: unknown) =>
        !cancelled &&
        setOutcome({ kind: "error", text: `The theme could not be stored in the desktop settings: ${e instanceof Error ? e.message : String(e)}` }),
    );
    return () => {
      cancelled = true;
    };
  }, [backend, loaded, busy, saved, preference]);

  const save = async (next: AppSettings, what: string) => {
    if (!backend) return false;
    setBusy(true);
    setOutcome(null);
    try {
      const stored = await backend.saveSettings(next);
      setSaved(stored);
      setOrch(stored.orch_path);
      setOutcome({ kind: "ok", text: `${what} saved.` });
      return true;
    } catch (e) {
      // Nothing was saved: show the reason and put the field back to what is stored.
      setOrch(saved.orch_path);
      setOutcome({ kind: "error", text: `${what} was not saved: ${e instanceof Error ? e.message : String(e)}` });
      return false;
    } finally {
      setBusy(false);
    }
  };

  return {
    backend,
    // The control shows the theme on screen; `saved` is what the shell stores.
    saved: { ...saved, theme: preference },
    loaded,
    loadWarning,
    detected,
    busy,
    outcome,
    orch,
    setOrch,
    changeTheme: async (value) => {
      if (!backend) {
        setPreference(value);
        return;
      }
      if (await save({ ...saved, theme: value }, "The theme")) setPreference(value);
    },
    changeNotifications: async (on) => {
      if (on && typeof Notification !== "undefined" && Notification.permission === "default") {
        await Notification.requestPermission();
      }
      await save({ ...saved, theme: preference, notifications: on }, "The notification setting");
    },
    saveOrchPath: async () => {
      await save({ ...saved, theme: preference, orch_path: orch.trim() }, "The orch path");
    },
  };
}
