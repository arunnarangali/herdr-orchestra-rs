import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../api/contract";
import type { AttentionItem } from "../api/types";
import { startFakeServer, type FakeServer } from "../test/fakeServer";
import { renderApp } from "../test/render";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  server.respond("GET", "/status", 200, { ...samples.status, branch: "orch/ui", run_state: "running" });
});
afterEach(() => server.close());

const item = (o: Partial<AttentionItem> = {}): AttentionItem => ({
  id: "a-1",
  kind: "held",
  task: "M2",
  agent: null,
  title: "M2 is held",
  detail: "touches the schema",
  prompt: null,
  since: "2026-11-15T12:00:00Z",
  actions: [],
  ...o,
});

describe("command palette", () => {
  it("Ctrl+K opens it with focus in the search, listing navigation and actions", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    const search = within(dialog).getByRole("combobox");
    expect(search).toHaveFocus();
    const options = within(dialog).getAllByRole("option").map((o) => o.textContent);
    for (const label of ["Overview", "Help", "Notifications / Attention Center", "Toggle light / dark theme", "Open notifications"]) {
      expect(options.some((o) => o?.startsWith(label))).toBe(true);
    }
  });

  it("filters as you type, and says so when nothing matches", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("settin");
    expect(within(dialog).getAllByRole("option")).toHaveLength(1);
    expect(within(dialog).getByRole("option")).toHaveTextContent("Settings");
    await user.keyboard("zzz");
    expect(within(dialog).queryByRole("option")).not.toBeInTheDocument();
    expect(within(dialog).getByText("No matching commands")).toBeInTheDocument();
  });

  it("Enter runs the highlighted command and closes; the arrows move the highlight", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.keyboard("{Control>}k{/Control}");
    await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("go to");
    await user.keyboard("{ArrowDown}{ArrowDown}"); // Projects
    expect(screen.getAllByRole("option")[2]).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{Enter}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Projects & Workspaces" })).toBeInTheDocument();
  });

  it("runs an action: the theme toggle", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.keyboard("{Control>}k{/Control}");
    await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("toggle{Enter}");
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });

  it("Escape closes it and focus returns to where it was; Ctrl+K again also closes", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    const opener = screen.getByRole("button", { name: /Search/ });
    opener.focus();
    await user.click(opener);
    await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
    await user.keyboard("{Control>}k{/Control}");
    await screen.findByRole("dialog");
    await user.keyboard("{Control>}k{/Control}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps Tab inside the dialog", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.tab();
    await user.tab();
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
  });
});

describe("notification drawer", () => {
  it("the bell shows the unread count and opens the drawer with the waiting items", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    expect(screen.getByRole("button", { name: "Notifications" })).toBeInTheDocument();
    act(() => server.emit(1, "attention", { op: "add", item: item() }));
    const alert = await screen.findByRole("alertdialog");
    await user.click(within(alert).getByRole("button", { name: "Dismiss" }));
    const bell = await screen.findByRole("button", { name: "Notifications, 1 need attention" });
    expect(bell).toHaveTextContent("1");

    await user.click(bell);
    const drawer = await screen.findByRole("dialog", { name: "Notifications" });
    expect(within(drawer).getByText("M2 is held")).toBeInTheDocument();
    expect(within(drawer).getByText("touches the schema")).toBeInTheDocument();
    expect(within(drawer.querySelector(".drawer__head") as HTMLElement).getByRole("button", { name: "Close notifications" })).toHaveFocus();
  });

  it("an empty drawer says all is clear; Escape closes it and focus returns to the bell", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    const bell = screen.getByRole("button", { name: "Notifications" });
    await user.click(bell);
    const drawer = await screen.findByRole("dialog", { name: "Notifications" });
    expect(within(drawer).getByText(/All clear/)).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(bell).toHaveFocus();
  });

  it("traps Tab, and its footer opens the Attention Center", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.click(screen.getByRole("button", { name: "Notifications" }));
    const drawer = await screen.findByRole("dialog", { name: "Notifications" });
    await user.tab();
    expect(drawer).toContainElement(document.activeElement as HTMLElement);
    await user.tab();
    expect(drawer).toContainElement(document.activeElement as HTMLElement);
    await user.click(within(drawer).getByRole("button", { name: /Open Attention Center/ }));
    expect(await screen.findByRole("heading", { name: "Notifications / Attention Center" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("is opened from the palette too", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await user.keyboard("{Control>}k{/Control}");
    await screen.findByRole("dialog", { name: "Command palette" });
    await user.keyboard("open notif{Enter}");
    expect(await screen.findByRole("dialog", { name: "Notifications" })).toBeInTheDocument();
  });
});

describe("frame", () => {
  it("shows the real branch, connection, run state and counts, and no demo badge", async () => {
    renderApp({ route: "/overview", initial: server });
    expect(await screen.findByText("orch/ui")).toBeInTheDocument(); // branch chip
    const bar = screen.getByRole("contentinfo");
    // /status and /health are separate requests: either may answer first
    await waitFor(() => {
      expect(bar).toHaveTextContent("Run: Running");
      expect(bar).toHaveTextContent("Connected");
    });
    expect(bar).toHaveTextContent(/milestones \d+\/\d+ merged/);
    expect(bar).toHaveTextContent("nothing needs you");
    expect(screen.queryByText(/Demo Data Layer/i)).not.toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Connection: Connected" })).toBeInTheDocument();
  });

  it("the driver pill reads the event manager", async () => {
    server.respond("GET", "/status", 200, { ...samples.status, event_manager: { running: true, holder: "orch autopilot" } });
    renderApp({ route: "/overview", initial: server });
    expect(await screen.findByText("Driver: orch autopilot · Running")).toBeInTheDocument();
  });

  it("the menu button opens the navigation overlay (small window) and Escape closes it", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    const toggle = screen.getByRole("button", { name: "Toggle navigation" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(document.querySelector(".app")).toHaveClass("app--nav-open");
    await user.keyboard("{Escape}");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveFocus();
    await user.click(toggle);
    await user.click(within(screen.getByRole("navigation", { name: "Main" })).getByRole("link", { name: "Reports" }));
    expect(toggle).toHaveAttribute("aria-expanded", "false");
  });

  it("opening the palette or the drawer closes the navigation overlay, so the dialog is on top", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    const toggle = screen.getByRole("button", { name: "Toggle navigation" });
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.keyboard("{Control>}k{/Control}");
    expect(await screen.findByRole("dialog", { name: "Command palette" })).toBeInTheDocument();
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(document.querySelector(".nav-backdrop")).toBeNull();
    await user.keyboard("{Escape}");

    await user.click(toggle);
    await user.click(screen.getByRole("button", { name: "Notifications" }));
    expect(await screen.findByRole("dialog", { name: "Notifications" })).toBeInTheDocument();
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.keyboard("{Escape}");
    expect(screen.getByRole("button", { name: "Notifications" })).toHaveFocus();
  });
});
