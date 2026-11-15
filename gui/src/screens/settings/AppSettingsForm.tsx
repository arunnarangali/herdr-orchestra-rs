import { useEffect, useId, useState } from "react";
import { useTheme, THEME_CHOICES, type ThemePreference } from "../../app/theme";
import { getBackend, type AppSettings } from "../projects/backend";

type Outcome = { kind: "ok" | "error"; text: string } | null;

const DEFAULTS: AppSettings = { theme: "system", notifications: false, orch_path: "" };

/**
 * The application's own settings. In the desktop app they are saved by the shell and survive a
 * restart; a save that fails says why and leaves the old value. In a browser only the theme is
 * offered, because only the theme can be saved there (in this browser).
 */
export function AppSettingsForm() {
  const backend = getBackend();
  const { preference, setPreference } = useTheme();
  const ids = { theme: useId(), notify: useId(), orch: useId() };
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
        setPreference(value.theme);
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

  const save = async (next: AppSettings, what: string) => {
    if (!backend) return;
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

  const changeTheme = async (value: ThemePreference) => {
    if (!backend) {
      setPreference(value);
      return;
    }
    if (await save({ ...saved, theme: value }, "The theme")) setPreference(value);
  };

  const changeNotifications = async (on: boolean) => {
    if (on && typeof Notification !== "undefined" && Notification.permission === "default") {
      await Notification.requestPermission();
    }
    await save({ ...saved, notifications: on }, "The notification setting");
  };

  return (
    <section className="panel" aria-label="App settings">
      <h2 className="panel-title">App settings</h2>
      {!backend && (
        <p className="muted" role="note">
          Running in a browser: the theme is remembered by this browser. Notifications and the <code>orch</code> path are
          saved by the desktop app only, so they are not offered here.
        </p>
      )}
      {loadWarning && (
        <p className="st-msg st-msg--error" role="alert">
          {loadWarning}
        </p>
      )}
      {!loaded && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {loaded && (
        <div className="st-form">
          <div className="field">
            <label htmlFor={ids.theme}>Theme</label>
            <select
              id={ids.theme}
              value={backend ? saved.theme : preference}
              disabled={busy}
              onChange={(e) => void changeTheme(e.target.value as ThemePreference)}
            >
              {THEME_CHOICES.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
          </div>

          {backend && (
            <>
              <div className="field st-check">
                <input
                  id={ids.notify}
                  type="checkbox"
                  checked={saved.notifications}
                  disabled={busy}
                  onChange={(e) => void changeNotifications(e.target.checked)}
                />
                <label htmlFor={ids.notify}>Desktop notifications for questions, holds and the end of a run</label>
              </div>

              <div className="field">
                <label htmlFor={ids.orch}>Path of the orch binary</label>
                <input id={ids.orch} value={orch} placeholder="Empty: use ORCH_BIN or the PATH" onChange={(e) => setOrch(e.target.value)} spellCheck={false} />
                <p className="muted">
                  {detected ? (
                    <>
                      Found now: <code>{detected}</code>.
                    </>
                  ) : (
                    "orch was not found: set its path here, or put it on the PATH."
                  )}{" "}
                  Used the next time a project's server starts.
                </p>
                <div>
                  <button
                    type="button"
                    className="button"
                    disabled={busy || orch === saved.orch_path}
                    onClick={() => void save({ ...saved, orch_path: orch.trim() }, "The orch path")}
                  >
                    Save path
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      )}
      {outcome && (
        <p className={outcome.kind === "ok" ? "st-msg st-msg--ok" : "st-msg st-msg--error"} role={outcome.kind === "ok" ? "status" : "alert"}>
          {outcome.text}
        </p>
      )}
    </section>
  );
}
