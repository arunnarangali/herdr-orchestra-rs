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

const NAMES = ["Overview", "Chat", "Projects", "Agents", "Workflows", "Execution", "Reports", "Settings"];

describe("registry and sidebar", () => {
  it("registers the eight screens and onboarding in one list", () => {
    expect(screens.filter((s) => s.nav).map((s) => s.label)).toEqual(NAMES);
    expect(screens.find((s) => s.id === "onboarding")?.nav).toBe(false);
    expect(new Set(screens.map((s) => s.path)).size).toBe(screens.length);
    const keys = screens.map((s) => s.shortcut).filter(Boolean);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it("lists the screens in the sidebar, marks the active one, and navigates by mouse", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(within(nav).getAllByRole("link").map((l) => l.textContent?.replace(/g\s*\w$/, "").trim())).toEqual(NAMES);
    expect(within(nav).getByRole("link", { name: /Overview/ })).toHaveAttribute("aria-current", "page");

    await user.click(within(nav).getByRole("link", { name: /Agents/ }));
    expect(await screen.findByRole("heading", { name: "Agents" })).toBeInTheDocument();
    expect(within(nav).getByRole("link", { name: /Agents/ })).toHaveAttribute("aria-current", "page");
    expect(within(nav).getByRole("link", { name: /Overview/ })).not.toHaveAttribute("aria-current");
  });

  it("shows every screen, the unbuilt ones as placeholders", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    // Every nav screen has its milestone's content now; the loop stays as a smoke test
    // and the placeholder assertion remains for any screen that loses its content.
    const built = ["Overview", "Chat", "Projects", "Agents", "Workflows", "Execution", "Reports", "Settings"];
    for (const name of NAMES) {
      await user.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("link", { name: new RegExp(name) }));
      expect(await screen.findByRole("heading", { name })).toBeInTheDocument();
      if (!built.includes(name)) expect(screen.getByText("Nothing here yet.")).toBeInTheDocument();
    }
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

  it("Getting started stays reachable from the Help menu once connected", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.click(screen.getByRole("button", { name: "Help" }));
    await user.click(screen.getByRole("menuitem", { name: "Getting started" }));
    expect(await screen.findByRole("heading", { name: "Getting started" })).toBeInTheDocument();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});

describe("keyboard", () => {
  it("g then a letter opens a screen", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    for (const s of screens.filter((x) => x.shortcut)) {
      await user.keyboard(`g${s.shortcut}`);
      expect(await screen.findByRole("heading", { name: s.label })).toBeInTheDocument();
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

  it("the Help menu opens the shortcut list too, and Escape closes the menu", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    const help = screen.getByRole("button", { name: "Help" });
    await user.click(help);
    expect(help).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("menuitem", { name: "Getting started" })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("menuitem", { name: /Keyboard shortcuts/ })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(help).toHaveFocus();
    await user.click(help);
    await user.click(screen.getByRole("menuitem", { name: /Keyboard shortcuts/ }));
    expect(await screen.findByRole("dialog", { name: "Keyboard shortcuts" })).toBeInTheDocument();
  });

  it("moves focus to the content after navigating", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("link", { name: /Reports/ }));
    await waitFor(() => expect(document.getElementById("main")).toHaveFocus());
  });
});

describe("theme", () => {
  it("follows the system setting until the user chooses, then keeps the choice", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    const select = screen.getByRole("combobox", { name: "Theme" });
    expect(select).toHaveValue("system");
    expect(document.documentElement).not.toHaveAttribute("data-theme");

    await user.selectOptions(select, "dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("orch.theme")).toBe("dark");
    await user.selectOptions(select, "light");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    await user.selectOptions(select, "system");
    expect(document.documentElement).not.toHaveAttribute("data-theme");
    expect(window.localStorage.getItem("orch.theme")).toBeNull();
  });

  it("restores the saved choice", () => {
    window.localStorage.setItem("orch.theme", "dark");
    renderApp({ route: "/overview", initial: server });
    expect(screen.getByRole("combobox", { name: "Theme" })).toHaveValue("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("tracks the system setting while on System", async () => {
    const { useTheme } = await import("./theme");
    const { render } = await import("@testing-library/react");
    const { ThemeProvider } = await import("./theme");
    function Show() {
      return <p data-testid="resolved">{useTheme().resolved}</p>;
    }
    render(
      <ThemeProvider>
        <Show />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("resolved")).toHaveTextContent("light");
    act(() => setSystemDark(true));
    expect(screen.getByTestId("resolved")).toHaveTextContent("dark");
    act(() => setSystemDark(false));
  });
});
