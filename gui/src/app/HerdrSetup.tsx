import { useCallback, useEffect, useState } from "react";
import { Button } from "../ui/Button";
import type { HerdrFixPlan, HerdrInfo, Shell } from "./shell";

/** Herdr's official site: where the developer goes when the app cannot install from here. */
export const HERDR_SITE = "https://herdr.dev";

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * How the app's Herdr stands, from the shell (which starts Herdr's server first when Herdr is
 * installed and not running). A shell without `getHerdrInfo` is read through `findHerdr`.
 * `null` while asking, and without a shell.
 */
export function useHerdr(shell: Shell | null, again: unknown = null) {
  const [info, setInfo] = useState<HerdrInfo | null>(null);
  const [nonce, setNonce] = useState(0);
  const refresh = useCallback(() => setNonce((n) => n + 1), []);

  useEffect(() => {
    setInfo(null);
    if (!shell) return;
    let live = true;
    const ask: Promise<HerdrInfo> = shell.getHerdrInfo
      ? shell.getHerdrInfo()
      : shell.findHerdr().then((path) => ({ ...RUNNING, path }));
    ask.then(
      (i) => live && setInfo(i),
      // a shell that can only look for Herdr: not found means install it (from herdr.dev)
      (e: unknown) => live && setInfo({ ...MISSING, error: messageOf(e) }),
    );
    return () => {
      live = false;
    };
  }, [shell, nonce, again]);

  return { info, setInfo, refresh };
}

const RUNNING: HerdrInfo = { state: "running", installed: true, path: null, version: null, running: true, managed: false, too_old: false, error: null, fix: null };
const MISSING: HerdrInfo = { state: "missing", installed: false, path: null, version: null, running: false, managed: false, too_old: false, error: null, fix: "install" };

/** One sentence on a running Herdr: where it is and whether the app stops it on close. */
export function herdrRunningText(info: HerdrInfo): string {
  const which = `Herdr${info.version ? ` ${info.version}` : ""}${info.path ? ` (${info.path})` : ""}`;
  return info.managed
    ? `${which} runs in the background: the app started it and stops it when you close the app.`
    : `${which} was already running: the app uses it and leaves it running when you close the app.`;
}

const ACTION_WORD = { install: "Install Herdr", update: "Update Herdr" } as const;

/**
 * What to do about a Herdr that is not running, in plain words: the reason, the fix the shell
 * offers (Install or Update, shown in full before it runs, and run only on a click), the
 * official site, and Check again. The command and script shown are the shell's own plan, so
 * what is shown is exactly what runs. Renders nothing for a running Herdr.
 */
export function HerdrSetup({
  shell,
  info,
  onChange,
  onRecheck,
  embedded = false,
}: {
  shell: Shell | null;
  info: HerdrInfo;
  /** A fix ran: Herdr as it stands now. */
  onChange: (info: HerdrInfo) => void;
  onRecheck: () => void;
  /** Inside a screen that already says why and has its own Check again. */
  embedded?: boolean;
}) {
  const [plan, setPlan] = useState<HerdrFixPlan | null>(null);
  const [busy, setBusy] = useState<"prepare" | "run" | null>(null);
  const [note, setNote] = useState<string | null>(null);

  if (info.state === "running") return null;

  const openSite = async () => {
    try {
      if (shell?.openHerdrUrl) await shell.openHerdrUrl();
      else window.open(HERDR_SITE, "_blank", "noopener");
      return true;
    } catch {
      return false;
    }
  };
  const verb = info.fix === "update" ? "update" : "install";
  const fallback = async (why: string) => {
    const opened = await openSite();
    setNote(
      `${why} ${opened ? `${HERDR_SITE} is open in your browser` : `Open ${HERDR_SITE} in your browser`}: follow its steps to ${verb} Herdr, then choose Check again.`,
    );
  };

  const prepare = async () => {
    if (!info.fix) return;
    setNote(null);
    if (!shell?.prepareHerdrFix || !shell.runHerdrFix) {
      await fallback(`The app cannot ${verb} Herdr from here.`);
      return;
    }
    setBusy("prepare");
    try {
      setPlan(await shell.prepareHerdrFix(info.fix));
    } catch (e) {
      await fallback(`The app could not prepare the ${verb} (${messageOf(e)}).`);
    } finally {
      setBusy(null);
    }
  };

  const run = async () => {
    if (!plan || !shell?.runHerdrFix) return;
    setBusy("run");
    setNote(null);
    try {
      const next = await shell.runHerdrFix(plan.token);
      setPlan(null);
      if (next.state !== "running") setNote(next.error);
      onChange(next);
    } catch (e) {
      setPlan(null);
      await fallback(messageOf(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="herdr-setup" role="group" aria-label="Herdr setup" data-herdr-state={info.state}>
      {!embedded && <p className="herdr-setup__why">{info.error ?? "Herdr is not running."}</p>}
      {plan ? (
        <div className="herdr-setup__plan" role="region" aria-label={`${ACTION_WORD[plan.action]}: what will run`}>
          {plan.script ? (
            <>
              <p>
                This runs Herdr&rsquo;s official installer from {HERDR_SITE} (<code>{plan.command}</code>). It is shown here exactly as it
                will run; nothing runs until you choose Run it.
              </p>
              <pre className="herdr-setup__script" tabIndex={0} aria-label="Installer script">
                {plan.script}
              </pre>
            </>
          ) : (
            <p>
              This runs Herdr&rsquo;s own updater: <code>{plan.command}</code>. Nothing runs until you choose Run it.
            </p>
          )}
          <div className="herdr-setup__actions">
            <Button tone="emerald" solid disabled={busy !== null} onClick={() => void run()}>
              {busy === "run" ? (plan.action === "install" ? "Installing…" : "Updating…") : "Run it"}
            </Button>
            <Button disabled={busy !== null} onClick={() => setPlan(null)}>
              Cancel
            </Button>
          </div>
        </div>
      ) : (
        <div className="herdr-setup__actions">
          {info.fix && (
            <Button tone="emerald" solid disabled={busy !== null} onClick={() => void prepare()}>
              {busy === "prepare" ? "Preparing…" : ACTION_WORD[info.fix]}
            </Button>
          )}
          <Button disabled={busy !== null} onClick={() => void openSite().then((ok) => setNote(ok ? `${HERDR_SITE} is open in your browser.` : `Open ${HERDR_SITE} in your browser.`))}>
            Open herdr.dev
          </Button>
          {!embedded && (
            <Button disabled={busy !== null} onClick={onRecheck}>
              Check again
            </Button>
          )}
        </div>
      )}
      {note && (
        <p className="herdr-setup__note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
