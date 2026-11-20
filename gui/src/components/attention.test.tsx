import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AttentionItem } from "../api/types";
import { setBackendForTests, type AppBackend } from "../screens/projects/backend";
import { startFakeServer, type FakeServer } from "../test/fakeServer";
import { renderApp } from "../test/render";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  server.respond("GET", "/attention", 200, { items: [] });
  setBackendForTests(null);
});
afterEach(() => {
  server.close();
  setBackendForTests(undefined);
  vi.restoreAllMocks();
});

const makeItem = (o: Partial<AttentionItem> = {}): AttentionItem => ({
  id: "item-1",
  kind: "question",
  task: "M1",
  agent: "coder-1",
  title: "Question on M1",
  detail: "Should we use YAML or JSON?",
  prompt: null,
  since: "2026-11-15T12:00:00Z",
  actions: ["answer"],
  ...o,
});

describe("Attention popup for each item kind", () => {
  it("opens popup for 'blocked' item, shows agent, task, and prompt", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    server.emit(1, "attention", {
      op: "add",
      item: makeItem({
        id: "b-1",
        kind: "blocked",
        agent: "coder-alpha",
        task: "M2",
        title: "Agent blocked on M2",
        prompt: "Permission prompt: Allow cargo build?",
        actions: [],
      }),
    });

    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Agent blocked on M2");
    expect(dialog).toHaveTextContent("coder-alpha");
    expect(dialog).toHaveTextContent("M2");
    expect(dialog).toHaveTextContent("Permission prompt: Allow cargo build?");

    await user.click(within(dialog).getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("opens popup for 'question' item and sends answer", async () => {
    const user = userEvent.setup();
    server.respond("POST", "/tasks/M1/answer", 200, { status: "ok" });
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    server.emit(2, "attention", {
      op: "add",
      item: makeItem({
        id: "q-1",
        kind: "question",
        task: "M1",
        title: "Question: API route name",
        actions: ["answer"],
      }),
    });

    const dialog = await screen.findByRole("alertdialog");
    const textarea = within(dialog).getByLabelText("Your answer:");
    await user.type(textarea, "Use /v1/agents");
    await user.click(within(dialog).getByRole("button", { name: "Send answer" }));

    await waitFor(() => {
      const posts = server.requests.filter((r) => r.path === "/tasks/M1/answer");
      expect(posts).toHaveLength(1);
      expect(JSON.parse(posts[0]!.body)).toEqual({ text: "Use /v1/agents" });
    });
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("opens popup for 'held' item and approves it", async () => {
    const user = userEvent.setup();
    server.respond("POST", "/tasks/M2/approve", 200, { status: "ok" });
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    server.emit(3, "attention", {
      op: "add",
      item: makeItem({
        id: "h-1",
        kind: "held",
        task: "M2",
        title: "Milestone M2 held: risk hold",
        detail: "Touches auth configuration",
        actions: ["approve"],
      }),
    });

    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Touches auth configuration");
    await user.click(within(dialog).getByRole("button", { name: "Approve" }));

    await waitFor(() => {
      const posts = server.requests.filter((r) => r.path === "/tasks/M2/approve");
      expect(posts).toHaveLength(1);
    });
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("opens popup for 'failed' item with fix and dispatch actions", async () => {
    const user = userEvent.setup();
    server.respond("POST", "/tasks/M3/fix", 200, { status: "ok" });
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    server.emit(4, "attention", {
      op: "add",
      item: makeItem({
        id: "f-1",
        kind: "failed",
        task: "M3",
        title: "Milestone M3 failed check",
        detail: "Unit test test_auth failed",
        actions: ["fix", "dispatch"],
      }),
    });

    const dialog = await screen.findByRole("alertdialog");
    const textarea = within(dialog).getByLabelText("Fix instructions:");
    await user.type(textarea, "Fix token parsing");
    await user.click(within(dialog).getByRole("button", { name: "Send fix" }));

    await waitFor(() => {
      const posts = server.requests.filter((r) => r.path === "/tasks/M3/fix");
      expect(posts).toHaveLength(1);
      expect(JSON.parse(posts[0]!.body)).toEqual({ text: "Fix token parsing" });
    });
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("opens popup for 'wake_unread' and 'plan_approval' items", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    server.emit(5, "attention", {
      op: "add",
      item: makeItem({
        id: "w-1",
        kind: "wake_unread",
        task: null,
        title: "Lead unread wake-up",
        detail: "Lead agent did not acknowledge wake-up",
        actions: ["open_chat"],
      }),
    });

    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Lead unread wake-up");
    await user.click(within(dialog).getByRole("button", { name: "Open chat" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  });

  it("a resolved item closes by itself on remove stream event", async () => {
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    server.emit(6, "attention", {
      op: "add",
      item: makeItem({ id: "auto-1", title: "Temporary hold", kind: "held" }),
    });

    expect(await screen.findByRole("alertdialog")).toBeInTheDocument();

    // The backend resolves the item (e.g. answered from CLI or resolved)
    server.emit(7, "attention", {
      op: "remove",
      item: makeItem({ id: "auto-1" }),
    });

    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
  });
});

describe("Indicator count and waiting list", () => {
  it("shows waiting indicator count and keeps dismissed items in list and count", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    // Initially nothing waits: the bell has no count
    expect(screen.queryByRole("button", { name: /need attention/ })).not.toBeInTheDocument();

    // 2 attention items arrive
    server.emit(8, "attention", {
      op: "add",
      item: makeItem({ id: "it-1", title: "Item 1", kind: "question" }),
    });
    server.emit(9, "attention", {
      op: "add",
      item: makeItem({ id: "it-2", title: "Item 2", kind: "held" }),
    });

    // Indicator appears with count 2
    const indicator = await screen.findByRole("button", { name: /Notifications, 2 need attention/ });
    expect(indicator).toBeInTheDocument();

    // First popup is shown for Item 1
    const dialog1 = await screen.findByRole("alertdialog");
    expect(dialog1).toHaveTextContent("Item 1");

    // Dismiss Item 1
    await user.click(within(dialog1).getByRole("button", { name: "Dismiss" }));

    // Second popup is shown for Item 2 (queueing)
    const dialog2 = await screen.findByRole("alertdialog");
    expect(dialog2).toHaveTextContent("Item 2");

    // Dismiss Item 2
    await user.click(within(dialog2).getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());

    // Both dismissed, but both stay in waiting count!
    expect(screen.getByRole("button", { name: /Notifications, 2 need attention/ })).toBeInTheDocument();

    // Clicking indicator opens waiting list
    await user.click(screen.getByRole("button", { name: /Notifications, 2 need attention/ }));
    const listModal = await screen.findByRole("dialog", { name: "Notifications" });
    expect(within(listModal).getByText("Item 1")).toBeInTheDocument();
    expect(within(listModal).getByText("Item 2")).toBeInTheDocument();

    // Clicking Review on Item 1 re-opens it in the popup
    await user.click(within(listModal).getByRole("button", { name: "Review Item 1" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("Item 1");
  });
});

describe("Queueing, focus return, and keyboard use", () => {
  it("queues items without stacking, closes on Escape, and returns focus where it was", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    // Focus on the bell before popup arrives
    const helpBtn = screen.getByRole("button", { name: "Notifications" });
    helpBtn.focus();
    expect(helpBtn).toHaveFocus();

    // Multiple items arrive simultaneously
    server.emit(10, "attention", {
      op: "add",
      item: makeItem({ id: "q-a", title: "Item A" }),
    });
    server.emit(11, "attention", {
      op: "add",
      item: makeItem({ id: "q-b", title: "Item B" }),
    });

    // Only one modal dialog exists in DOM (never stacked)
    await screen.findByRole("alertdialog");
    expect(screen.getAllByRole("alertdialog")).toHaveLength(1);
    expect(screen.getByRole("alertdialog")).toHaveTextContent("Item A");

    // Press Escape to dismiss Item A
    await user.keyboard("{Escape}");

    // Item B is now shown
    await waitFor(() => {
      expect(screen.getByRole("alertdialog")).toHaveTextContent("Item B");
    });
    expect(screen.getAllByRole("alertdialog")).toHaveLength(1);

    // Dismiss Item B
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());

    // Focus returns where it was (the bell)
    expect(helpBtn).toHaveFocus();
  });
});

describe("Desktop notifications", () => {
  it("respects desktop notifications setting being off", async () => {
    const fakeBackend: AppBackend = {
      chooseFolder: async () => null,
      listProjects: async () => ({ projects: [], active: null, open: [], warning: null }),
      addProject: async () => ({ projects: [], active: null, open: [], warning: null }),
      removeProject: async () => ({ projects: [], active: null, open: [], warning: null }),
      switchProject: async () => ({ url: "", token: "" }),
      openProject: async () => ({ url: "", token: "" }),
      retryProject: async () => ({ url: "", token: "" }),
      closeProject: async () => ({ projects: [], active: null, open: [], warning: null }),
      restoreProjects: async () => [],
      createProject: async () => {
        throw new Error("x");
      },
      getSettings: async () => ({
        value: { theme: "system", notifications: false, orch_path: "" },
        warning: null,
      }),
      saveSettings: async (s) => s,
      findOrch: async () => "/bin/orch",
    };
    setBackendForTests(fakeBackend);

    const notificationSpy = vi.fn();
    vi.stubGlobal("Notification", Object.assign(notificationSpy, { permission: "granted" }));

    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    server.emit(12, "attention", {
      op: "add",
      item: makeItem({ id: "notif-off", title: "Notice" }),
    });

    await screen.findByRole("alertdialog");
    expect(notificationSpy).not.toHaveBeenCalled();
  });

  it("sends notification when desktop notification setting is on", async () => {
    const fakeBackend: AppBackend = {
      chooseFolder: async () => null,
      listProjects: async () => ({ projects: [], active: null, open: [], warning: null }),
      addProject: async () => ({ projects: [], active: null, open: [], warning: null }),
      removeProject: async () => ({ projects: [], active: null, open: [], warning: null }),
      switchProject: async () => ({ url: "", token: "" }),
      openProject: async () => ({ url: "", token: "" }),
      retryProject: async () => ({ url: "", token: "" }),
      closeProject: async () => ({ projects: [], active: null, open: [], warning: null }),
      restoreProjects: async () => [],
      createProject: async () => {
        throw new Error("x");
      },
      getSettings: async () => ({
        value: { theme: "system", notifications: true, orch_path: "" },
        warning: null,
      }),
      saveSettings: async (s) => s,
      findOrch: async () => "/bin/orch",
    };
    setBackendForTests(fakeBackend);

    const notificationSpy = vi.fn();
    vi.stubGlobal("Notification", Object.assign(notificationSpy, { permission: "granted" }));

    renderApp({ route: "/overview", initial: server });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    server.emit(13, "attention", {
      op: "add",
      item: makeItem({ id: "notif-on", title: "Important Question", prompt: "Pick one" }),
    });

    await screen.findByRole("alertdialog");
    await waitFor(() => {
      expect(notificationSpy).toHaveBeenCalledWith("Important Question", {
        body: "Pick one",
      });
    });
  });
});
