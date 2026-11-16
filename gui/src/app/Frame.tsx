import { useEffect, useRef, useState } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { AttentionIndicator, AttentionProvider } from "../components/Attention";
import { ConnectPanel } from "./ConnectPanel";
import { ProjectBar, ProjectProblem } from "./ProjectBar";
import { useProjects } from "./projects";
import { ConnectionBanner, ConnectionPill } from "./ConnectionStatus";
import { useConnection } from "./connection";
import { HelpMenu } from "./HelpMenu";
import { DEFAULT_PATH, ONBOARDING_PATH, screens } from "./routes";
import { ShortcutList, useShortcuts } from "./shortcuts";
import { THEME_CHOICES, useTheme, type ThemePreference } from "./theme";

/**
 * The frame every screen sits in: sidebar, top bar (connection state, theme, Help) and the
 * main area. It knows nothing about what a screen shows.
 */
export function Frame() {
  const { kind } = useConnection();
  const { multi, projects, selected, restoring } = useProjects();
  const current = projects.find((p) => p.path === selected);
  const { preference, setPreference } = useTheme();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const main = useRef<HTMLElement>(null);
  const first = useRef(true);
  const previousKind = useRef(kind);

  useShortcuts({ goTo: (p) => navigate(p), showHelp: () => setShortcutsOpen(true) });

  // Move focus to the content after navigating, so keyboard and screen-reader users start there.
  useEffect(() => {
    if (first.current) first.current = false;
    else main.current?.focus();
  }, [pathname]);

  // Once a project is chosen, leave the connect step for the first screen.
  useEffect(() => {
    if (previousKind.current === "unconfigured" && kind !== "unconfigured") {
      navigate(DEFAULT_PATH, { replace: true });
    }
    previousKind.current = kind;
  }, [kind, navigate]);

  const showConnect = kind === "unconfigured";
  // With several projects, the screens wait for the selected project's server.
  const waiting = multi && (restoring || (current !== undefined && current.status !== "ready"));

  return (
    <AttentionProvider>
      <div className="app">
        <a className="skip-link" href="#main">
          Skip to content
        </a>
        <aside className="sidebar">
          <div className="brand">herdr-orchestra</div>
          <nav aria-label="Main">
            <ul className="nav">
              {screens
                .filter((s) => s.nav)
                .map((s) => (
                  <li key={s.id}>
                    <NavLink to={s.path} className={({ isActive }) => (isActive ? "nav-link active" : "nav-link")}>
                      <span>{s.label}</span>
                      {s.shortcut && (
                        <span className="nav-keys" aria-hidden="true">
                          <kbd>g</kbd> <kbd>{s.shortcut}</kbd>
                        </span>
                      )}
                    </NavLink>
                  </li>
                ))}
            </ul>
          </nav>
        </aside>
        <div className="content">
          <ProjectBar />
          <header className="topbar">
            {multi && current && (
              <span className="current-project" title={current.path}>
                {current.name}
              </span>
            )}
            <ConnectionPill />
            <span className="spacer" />
            <div className="chat-entry-wrap">
              <AttentionIndicator />
            </div>
            <label className="theme-select">
              <span>Theme</span>
              <select value={preference} onChange={(e) => setPreference(e.target.value as ThemePreference)}>
                {THEME_CHOICES.map((c) => (
                  <option key={c.value} value={c.value}>
                    {c.label}
                  </option>
                ))}
              </select>
            </label>
            <HelpMenu onOnboarding={() => navigate(ONBOARDING_PATH)} onShortcuts={() => setShortcutsOpen(true)} />
          </header>
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
        <ShortcutList open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />
      </div>
    </AttentionProvider>
  );
}
