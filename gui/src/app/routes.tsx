import type { ComponentType } from "react";
import { AgentsScreen } from "../screens/agents";
import { ChatScreen } from "../screens/chat";
import { ExecutionScreen } from "../screens/execution";
import { HelpScreen } from "../screens/help";
import { NotificationsScreen } from "../screens/notifications";
import { OnboardingScreen } from "../screens/onboarding";
import { OverviewScreen } from "../screens/overview";
import { ProjectsScreen } from "../screens/projects";
import { ReportsScreen } from "../screens/reports";
import { SettingsScreen } from "../screens/settings";
import { WorkflowsScreen } from "../screens/workflows";
import type { IconName } from "../ui/Icon";

export interface ScreenDef {
  id: string;
  /** Route path. */
  path: string;
  /** The name in the sidebar and the command palette (the prototype's wording). */
  label: string;
  icon: IconName;
  Component: ComponentType;
  /** Listed in the sidebar. */
  nav: boolean;
  /** Second key of the `g` chord that opens it (`g` then `o` for Overview). */
  shortcut?: string;
}

/**
 * Every screen, registered in one place: the sidebar, the routes, the command palette, the
 * keyboard shortcuts and the shortcut list are all built from this list, in the prototype's
 * order. A screen milestone replaces the component inside its own `screens/<name>/` folder and
 * leaves this file alone.
 */
export const screens: readonly ScreenDef[] = [
  { id: "overview", path: "/overview", label: "Overview", icon: "grid", Component: OverviewScreen, nav: true, shortcut: "o" },
  { id: "chat", path: "/chat", label: "Chat / Home", icon: "chat", Component: ChatScreen, nav: true, shortcut: "c" },
  { id: "projects", path: "/projects", label: "Projects", icon: "folder", Component: ProjectsScreen, nav: true, shortcut: "p" },
  { id: "agents", path: "/agents", label: "Agents", icon: "robot", Component: AgentsScreen, nav: true, shortcut: "a" },
  { id: "workflows", path: "/workflows", label: "Workflows (Plan)", icon: "branch", Component: WorkflowsScreen, nav: true, shortcut: "w" },
  { id: "execution", path: "/execution", label: "Execution Monitor", icon: "activity", Component: ExecutionScreen, nav: true, shortcut: "e" },
  { id: "reports", path: "/reports", label: "Reports", icon: "chart", Component: ReportsScreen, nav: true, shortcut: "r" },
  { id: "settings", path: "/settings", label: "Settings", icon: "gear", Component: SettingsScreen, nav: true, shortcut: "s" },
  // Also the redirect target while no project has been chosen.
  { id: "onboarding", path: "/onboarding", label: "Onboarding", icon: "rocket", Component: OnboardingScreen, nav: true },
  { id: "help", path: "/help", label: "Help", icon: "help", Component: HelpScreen, nav: true, shortcut: "h" },
  // As in the prototype it is reached from the bell, the palette and `g n`, not the sidebar.
  {
    id: "notifications",
    path: "/notifications",
    label: "Notifications / Attention Center",
    icon: "bell",
    Component: NotificationsScreen,
    nav: false,
    shortcut: "n",
  },
];

export { DEFAULT_PATH, NOTIFICATIONS_PATH, ONBOARDING_PATH } from "./paths";
