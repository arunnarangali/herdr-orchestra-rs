import { Link } from "react-router-dom";
import { NotificationBell } from "../components/Attention";
import { Icon } from "../ui/Icon";
import { ConnectionPill } from "./ConnectionStatus";
import { ProjectBar } from "./ProjectBar";
import { DEFAULT_PATH } from "./paths";
import { useTheme } from "./theme";

export interface TopbarProps {
  navOpen: boolean;
  onToggleNav: (open: boolean) => void;
  onOpenPalette: () => void;
  /** The branch of the run on screen, when there is one. */
  branch: string | null;
  navToggleRef: React.RefObject<HTMLButtonElement | null>;
}

/** The top bar: menu button (small windows), brand, project tabs, branch, connection, bell, theme, search. */
export function Topbar({ navOpen, onToggleNav, onOpenPalette, branch, navToggleRef }: TopbarProps) {
  const { resolved, toggle } = useTheme();
  const light = resolved === "light";
  return (
    <header id="topbar" className="topbar">
      <button
        ref={navToggleRef}
        type="button"
        className="topbar__icon-btn topbar__nav-toggle"
        aria-label="Toggle navigation"
        aria-controls="sidebar"
        aria-expanded={navOpen}
        onClick={() => onToggleNav(!navOpen)}
      >
        <Icon name="menu" />
      </button>
      <Link className="topbar__brand" to={DEFAULT_PATH} aria-label="Herdr Orchestra home">
        <Icon name="pulse" />
        <span className="topbar__brand-name">Herdr Orchestra</span>
      </Link>
      <span className="topbar__sep" aria-hidden="true" />
      <ProjectBar />
      <span className="topbar__spacer" />
      {branch && (
        <span className="chip chip--branch" title="Current branch">
          <Icon name="branch" />
          <span>{branch}</span>
        </span>
      )}
      <ConnectionPill />
      <span className="topbar__sep" aria-hidden="true" />
      <NotificationBell />
      <button
        type="button"
        className="topbar__icon-btn"
        aria-label={light ? "Switch to dark theme" : "Switch to light theme"}
        aria-pressed={light}
        onClick={toggle}
      >
        <Icon name={light ? "sun" : "moon"} />
      </button>
      <button type="button" className="palette-btn" aria-haspopup="dialog" onClick={onOpenPalette}>
        <Icon name="search" />
        <span>Search</span>
        <kbd>Ctrl+K</kbd>
      </button>
    </header>
  );
}
