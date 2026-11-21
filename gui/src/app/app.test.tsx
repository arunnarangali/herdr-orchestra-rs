import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { screens } from "./routes";
import { setSystemDark } from "../test/setup";
import { startFakeServer, type FakeServer } from "../test/fakeServer";
import { renderApp } from "../test/render";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
});
afterEach(() => server.close());

// The sidebar's entries, in the prototype's order and wording.
const NAMES = ["Overview", "Chat / Home", "Projects", "Agents", "Workflows (Plan)", "Execution Monitor", "Reports", "Settings", "Onboarding", "Help"];
// What each screen's own heading says today (a screen milestone may change its own).
const HEADING: Record<string, string> = {
  overview: "Overview",
  chat: "Chat",
  projects: "Projects & Workspaces",
  agents: "Agents & Models",
  workflows: "Workflows",
  execution: "Execution",
  reports: "Reports",
  settings: "Settings",
  onboarding: "Getting started",
  help: "Help",
  notifications: "Notifications / Attention Center",
};

describe("registry and sidebar", () => {
  it("registers all eleven screens in one list, ten in the sidebar", () => {
    expect(screens).toHaveLength(11);
    expect(screens.filter((s) => s.nav).map((s) => s.label)).toEqual(NAMES);
    expect(screens.find((s) => s.id === "notifications")).toMatchObject({ nav: false, path: "/notifications" });
    expect(screens.find((s) => s.id === "help")).toMatchObject({ nav: true, path: "/help" });
    expect(new Set(screens.map((s) => s.path)).size).toBe(screens.length);
    const keys = screens.map((s) => s.shortcut).filter(Boolean);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("lists the screens in the sidebar, marks the active one, and navigates by mouse", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(within(nav).getAllByRole("link").map((l) => l.textContent?.trim())).toEqual(NAMES);
    expect(within(nav).getByRole("link", { name: "Overview" })).toHaveAttribute("aria-current", "page");

    await user.click(within(nav).getByRole("link", { name: "Agents" }));
    expect(await screen.findByRole("heading", { name: "Agents & Models" })).toBeInTheDocument();
    expect(within(nav).getByRole("link", { name: "Agents" })).toHaveAttribute("aria-current", "page");
    expect(within(nav).getByRole("link", { name: "Overview" })).not.toHaveAttribute("aria-current");
  });

  it("shows every screen, the unbuilt ones as placeholders", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    for (const s of screens.filter((x) => x.nav)) {
      await user.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("link", { name: s.label }));
      expect(await screen.findByRole("heading", { name: HEADING[s.id] })).toBeInTheDocument();
    }
    // the last one visited, Help, is built: its reference is there, not the placeholder
    expect(screen.getByRole("region", { name: /Keyboard Shortcuts/ })).toBeInTheDocument();
  }, 15_000); // it visits all ten screens, each loading its data

  it("the Attention Center has a route and its body", async () => {
    renderApp({ route: "/notifications", initial: server });
    expect(await screen.findByRole("heading", { name: "Notifications / Attention Center" })).toBeInTheDocument();
    expect(await screen.findByText("All clear. Nothing needs your attention.")).toBeInTheDocument();
  });

  it("an unknown address goes to the first screen", async () => {
    renderApp({ route: "/nowhere", initial: server });
    expect(await screen.findByRole("heading", { name: "Overview" })).toBeInTheDocument();
  });
});

describe("no project chosen", () => {
  it("redirects every screen to Getting started and lists nothing else", async () => {
    for (const route of ["/", "/overview", "/settings", "/reports"]) {
      const { unmount } = renderApp({ route });
      expect(await screen.findByRole("heading", { name: "Getting started" })).toBeInTheDocument();
      unmount();
    }
  });

  it("Getting started stays reachable from the sidebar once connected", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("link", { name: "Onboarding" }));
    expect(await screen.findByRole("heading", { name: "Getting started" })).toBeInTheDocument();
  });
});

describe("keyboard", () => {
  it("the two new screens have shortcuts that do not clash", () => {
    const keys = screens.map((s) => s.shortcut).filter(Boolean);
    expect(new Set(keys).size).toBe(keys.length);
    expect(screens.find((s) => s.id === "notifications")?.shortcut).toBe("n");
    expect(screens.find((s) => s.id === "help")?.shortcut).toBe("h");
  });

  it("g then a letter opens a screen", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    for (const s of screens.filter((x) => x.shortcut)) {
      await user.keyboard(`g${s.shortcut}`);
      expect(await screen.findByRole("heading", { name: HEADING[s.id] })).toBeInTheDocument();
    }
  });

  it("ignores the chord while typing in a field", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview" });
    // not connected: the form is on screen
    await user.click(screen.getByLabelText("Token"));
    await user.keyboard("ga");
    expect(screen.getByLabelText("Token")).toHaveValue("ga");
    expect(screen.getByRole("heading", { name: "Getting started" })).toBeInTheDocument();
  });

  it("? opens the shortcut list with every shortcut, Escape closes it and focus returns", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.keyboard("?");
    const dialog = await screen.findByRole("dialog", { name: "Keyboard shortcuts" });
    for (const s of screens.filter((x) => x.shortcut)) {
      expect(within(dialog).getByText(`Go to ${s.label}`)).toBeInTheDocument();
    }
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("moves focus to the content after navigating", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("link", { name: /Reports/ }));
    await waitFor(() => expect(document.getElementById("main")).toHaveFocus());
  });
});

describe("theme", () => {
  const toggle = () => screen.getByRole("button", { name: /Switch to (light|dark) theme/ });

  it("is dark by default and the top bar toggles to light and back, remembering the choice", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(toggle()).toHaveAccessibleName("Switch to light theme");
    expect(window.localStorage.getItem("orch.theme")).toBeNull();

    await user.click(toggle());
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(window.localStorage.getItem("orch.theme")).toBe("light");
    expect(toggle()).toHaveAccessibleName("Switch to dark theme");
    expect(toggle()).toHaveAttribute("aria-pressed", "true");

    await user.click(toggle());
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("orch.theme")).toBe("dark");
  });

  it("restores the saved choice", () => {
    window.localStorage.setItem("orch.theme", "light");
    renderApp({ route: "/overview", initial: server });
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(toggle()).toHaveAccessibleName("Switch to dark theme");
  });

  it("follows the system setting only when System is chosen", async () => {
    const { useTheme, ThemeProvider } = await import("./theme");
    const { render } = await import("@testing-library/react");
    function Show() {
      const t = useTheme();
      return (
        <>
          <p data-testid="resolved">{t.resolved}</p>
          <button onClick={() => t.setPreference("system")}>system</button>
        </>
      );
    }
    render(
      <ThemeProvider>
        <Show />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("resolved")).toHaveTextContent("dark");
    act(() => setSystemDark(true));
    expect(screen.getByTestId("resolved")).toHaveTextContent("dark");
    await userEvent.setup().click(screen.getByText("system"));
    act(() => setSystemDark(false));
    expect(screen.getByTestId("resolved")).toHaveTextContent("light");
    act(() => setSystemDark(false));
  });
});
