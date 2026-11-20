import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { AttentionProvider, useAttention } from "../components/Attention";
import { useResource } from "../ui/useResource";
import { ToastProvider, useToast } from "../ui/Toast";
import { ConnectPanel } from "./ConnectPanel";
import { CommandPalette, type Command } from "./CommandPalette";
import { ProjectProblem } from "./ProjectBar";
import { useProjects } from "./projects";
import { ConnectionBanner } from "./ConnectionStatus";
import { useConnection } from "./connection";
import { DEFAULT_PATH } from "./paths";
import { screens } from "./routes";
import { ShortcutList, useShortcuts } from "./shortcuts";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { useTheme } from "./theme";
import { Topbar } from "./Topbar";

/** The width at which the sidebar becomes an overlay (matches `styles/base.css`). */
const OVERLAY_MAX_PX = 640;

/**
 * The frame every screen sits in: top bar, sidebar, status bar, command palette, notification
 * drawer and toasts around the main area. It knows nothing about what a screen shows.
 */
export function Frame() {
  return (
    <ToastProvider>
      <AttentionProvider>
        <FrameBody />
      </AttentionProvider>
    </ToastProvider>
  );
}

function FrameBody() {
  const { kind } = useConnection();
  const { multi, projects, selected, restoring, select } = useProjects();
  const current = projects.find((p) => p.path === selected);
  const { resolved, toggle } = useTheme();
  const { waitingItems, openList, isListOpen, currentItem } = useAttention();
  const toast = useToast();
  const { recheck } = useConnection();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [navOpen, setNavOpen] = useState(false);
  const main = useRef<HTMLElement>(null);
  const navToggle = useRef<HTMLButtonElement>(null);
  const first = useRef(true);
  const previousKind = useRef(kind);

  const status = useResource((api, signal) => api.status({ signal }), [], { refreshOn: ["task-state", "run-state"], refreshDelayMs: 500 });
  const run = status.state === "ready" ? status.data : null;

  useShortcuts({ goTo: (p) => navigate(p), showHelp: () => setShortcutsOpen(true) });

  // Move focus to the content after navigating, so keyboard and screen-reader users start there.
  useEffect(() => {
    if (first.current) first.current = false;
    else main.current?.focus();
    setNavOpen(false);
  }, [pathname]);

  // Once a project is chosen, leave the connect step for the first screen.
  useEffect(() => {
    if (previousKind.current === "unconfigured" && kind !== "unconfigured") {
      navigate(DEFAULT_PATH, { replace: true });
    }
    previousKind.current = kind;
  }, [kind, navigate]);

  // A dialog (palette, drawer, attention popup, shortcut list) replaces the navigation overlay.
  const dialogOpen = paletteOpen || isListOpen || shortcutsOpen || currentItem !== null;
  useEffect(() => {
    if (dialogOpen) setNavOpen(false);
  }, [dialogOpen]);

  // Ctrl+K toggles the palette from anywhere (also while typing); Escape closes the overlay menu.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      } else if (e.key === "Escape" && navOpen && !e.defaultPrevented) {
        setNavOpen(false);
        navToggle.current?.focus();
      }
    };
    const onResize = () => window.innerWidth > OVERLAY_MAX_PX && setNavOpen(false);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", onResize);
    return () => {
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
    };
  }, [navOpen]);

  const commands = useMemo<Command[]>(
    () => [
      ...screens.map((s) => ({
        group: "Go to",
        label: s.label,
        icon: s.icon,
        hint: s.shortcut ? `g ${s.shortcut}` : undefined,
        run: () => navigate(s.path),
      })),
      ...(multi
        ? projects.map((p) => ({
            group: "Projects",
            label: `Switch to ${p.name}`,
            icon: "folder" as const,
            run: () => {
              select(p.path);
              toast(`Switched to ${p.name}`, { tone: "violet", icon: "folder" });
            },
          }))
        : []),
      { group: "Actions", label: "Toggle light / dark theme", icon: resolved === "dark" ? "sun" : "moon", hint: "theme", run: toggle },
      { group: "Actions", label: "Open notifications", icon: "bell", hint: `${waitingItems.length} waiting`, run: openList },
      { group: "Actions", label: "Keyboard shortcuts", icon: "keyboard", hint: "?", run: () => setShortcutsOpen(true) },
      { group: "Actions", label: "Check the connection again", icon: "sync", run: recheck },
    ],
    [navigate, multi, projects, select, toast, resolved, toggle, waitingItems.length, openList, recheck],
  );

  const showConnect = kind === "unconfigured";
  // With several projects, the screens wait for the selected project's server.
  const waiting = multi && (restoring || (current !== undefined && current.status !== "ready"));

  const em = run?.event_manager;
  const driver = em ? `Driver: ${em.holder ?? "orch"} · ${em.running ? "Running" : "Stopped"}` : "Driver: not connected";
  const closeNav = useCallback(() => setNavOpen(false), []);

  return (
    <div className={navOpen ? "app app--nav-open" : "app"}>
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <Topbar navOpen={navOpen} onToggleNav={setNavOpen} onOpenPalette={() => setPaletteOpen(true)} branch={run?.branch ?? null} navToggleRef={navToggle} />
      <Sidebar driver={driver} driverOk={em?.running ?? false} onNavigate={closeNav} />
      {navOpen && <div className="nav-backdrop" aria-hidden="true" onClick={closeNav} />}
      <div className="content">
        <ConnectionBanner />
        <main id="main" ref={main} tabIndex={-1} className="main">
          {multi && restoring && (
            <p role="status" className="muted">
              Opening your projects…
            </p>
          )}
          {waiting && !restoring && <ProjectProblem />}
          {!waiting && showConnect && <ConnectPanel />}
          {!waiting && <Outlet />}
        </main>
      </div>
      <StatusBar kind={kind} status={run} waiting={waitingItems.length} />
      <CommandPalette open={paletteOpen} commands={commands} onClose={() => setPaletteOpen(false)} />
      <ShortcutList open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />
    </div>
  );
}
