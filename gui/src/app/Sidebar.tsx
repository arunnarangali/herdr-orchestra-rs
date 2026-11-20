import { NavLink } from "react-router-dom";
import { Icon } from "../ui/Icon";
import { screens } from "./routes";

export interface SidebarProps {
  /** The driver pill's text, e.g. "Driver: alice · Running". */
  driver: string;
  driverTitle?: string;
  driverOk: boolean;
  /** Called after a link is followed (closes the overlay on a small window). */
  onNavigate: () => void;
}

/**
 * The primary navigation: brand block, the screens in the prototype's order, and the driver
 * status pill. 220px wide; an icon rail at 1000px; an overlay opened by the menu button at 640px.
 */
export function Sidebar({ driver, driverTitle, driverOk, onNavigate }: SidebarProps) {
  return (
    <aside id="sidebar" className="sidebar">
      <div className="side__brand">
        <Icon name="cube" />
        <div className="side__brand-text">
          <strong>HERDR</strong>
          <span>Orchestra</span>
        </div>
      </div>
      <nav className="side__nav" aria-label="Main">
        {screens
          .filter((s) => s.nav)
          .map((s) => (
            <NavLink key={s.id} to={s.path} className="nav__link" title={s.label} onClick={onNavigate}>
              <Icon name={s.icon} />
              <span>{s.label}</span>
            </NavLink>
          ))}
      </nav>
      <div className="side__foot">
        <div className={driverOk ? "driver-pill" : "driver-pill driver-pill--off"} role="status" title={driverTitle}>
          <Icon name="bolt" />
          <span>{driver}</span>
        </div>
      </div>
    </aside>
  );
}
