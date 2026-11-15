import type { ComponentType } from "react";
import { AgentsScreen } from "../screens/agents";
import { ExecutionScreen } from "../screens/execution";
import { OnboardingScreen } from "../screens/onboarding";
import { OverviewScreen } from "../screens/overview";
import { ProjectsScreen } from "../screens/projects";
import { ReportsScreen } from "../screens/reports";
import { SettingsScreen } from "../screens/settings";
import { WorkflowsScreen } from "../screens/workflows";

export interface ScreenDef {
  id: string;
  /** Route path. */
  path: string;
  label: string;
  Component: ComponentType;
  /** Listed in the sidebar. */
  nav: boolean;
  /** Second key of the `g` chord that opens it (`g` then `o` for Overview). */
  shortcut?: string;
}

/**
 * Every screen, registered in one place: the sidebar, the routes, the keyboard shortcuts and
 * the shortcut list are all built from this list. A screen milestone replaces the component
 * inside its own `screens/<name>/` folder and leaves this file alone.
 */
export const screens: readonly ScreenDef[] = [
  { id: "overview", path: "/overview", label: "Overview", Component: OverviewScreen, nav: true, shortcut: "o" },
  { id: "projects", path: "/projects", label: "Projects", Component: ProjectsScreen, nav: true, shortcut: "p" },
  { id: "agents", path: "/agents", label: "Agents", Component: AgentsScreen, nav: true, shortcut: "a" },
  { id: "workflows", path: "/workflows", label: "Workflows", Component: WorkflowsScreen, nav: true, shortcut: "w" },
  { id: "execution", path: "/execution", label: "Execution", Component: ExecutionScreen, nav: true, shortcut: "e" },
  { id: "reports", path: "/reports", label: "Reports", Component: ReportsScreen, nav: true, shortcut: "r" },
  { id: "settings", path: "/settings", label: "Settings", Component: SettingsScreen, nav: true, shortcut: "s" },
  // Reached from the Help menu, and the redirect target while no project has been chosen.
  { id: "onboarding", path: "/onboarding", label: "Getting started", Component: OnboardingScreen, nav: false },
];

export const ONBOARDING_PATH = "/onboarding";
export const DEFAULT_PATH = "/overview";
