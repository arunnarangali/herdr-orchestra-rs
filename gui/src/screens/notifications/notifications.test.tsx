import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AttentionItem } from "../../api/types";
import { setBackendForTests } from "../projects/backend";
import { startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  setBackendForTests(null);
});
afterEach(async () => {
  await server.close();
  setBackendForTests(undefined);
});

const item = (o: Partial<AttentionItem> = {}): AttentionItem => ({
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

const openWith = async (items: AttentionItem[]) => {
  server.respond("GET", "/attention", 200, { items });
  renderApp({ route: "/notifications", initial: server });
  expect(await screen.findByRole("heading", { name: "Notifications / Attention Center" })).toBeInTheDocument();
};

describe("the Attention Center", () => {
  it("says all clear when nothing needs attention", async () => {
    await openWith([]);
    expect(screen.getByText("Attention Center (0 Unresolved)")).toBeInTheDocument();
    expect(screen.getByText("All clear. Nothing needs your attention.")).toBeInTheDocument();
  });

  it("filters by kind and shows a clear empty state", async () => {
    const user = userEvent.setup();
    await openWith([
      item({ id: "q-1", title: "Question on M1" }),
      item({ id: "h-1", kind: "held", task: "M2", title: "Milestone M2 held", actions: ["approve"] }),
    ]);
    expect(await screen.findByText("Question on M1")).toBeInTheDocument();
    expect(screen.getByText("Milestone M2 held")).toBeInTheDocument();

    await user.click(within(screen.getByRole("group", { name: "Kind" })).getByRole("button", { name: /Risk Holds/ }));
    expect(screen.queryByText("Question on M1")).not.toBeInTheDocument();
    expect(screen.getByText("Milestone M2 held")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Inspect Diff/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Reject \/ Pause/ })).toBeDisabled();

    await user.click(screen.getByRole("tab", { name: /Resolved/ }));
    expect(screen.getByText("No notifications match these filters.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Reset filters" }));
    expect(await screen.findByText("Question on M1")).toBeInTheDocument();
  });

  it("answers from a card, updates the bell, and keeps the resolved history", async () => {
    const user = userEvent.setup();
    server.respond("POST", "/tasks/M1/answer", 200, { status: "ok" });
    await openWith([item()]);

    expect(await screen.findByRole("button", { name: /Notifications, 1 need attention/ })).toBeInTheDocument();
    expect(screen.getByRole("contentinfo")).toHaveTextContent("1 needs attention");
    const answer = screen.getByLabelText("Answer Question on M1");
    await user.type(answer, "Use JSON");
    await user.click(screen.getByRole("button", { name: "Send answer" }));

    await waitFor(() => {
      const posts = server.requests.filter((r) => r.path === "/tasks/M1/answer");
      expect(posts).toHaveLength(1);
      expect(JSON.parse(posts[0]!.body)).toEqual({ text: "Use JSON" });
    });
    await waitFor(() => expect(screen.queryByText("Question on M1")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.getByRole("contentinfo")).toHaveTextContent("nothing needs you");

    await user.click(screen.getByRole("tab", { name: /Resolved/ }));
    const histories = await screen.findAllByRole("heading", { name: /Resolved history/ });
    const group = histories.find((h) => h.id === "nc-resolved-title")!.closest("section")!;
    expect(within(group).getByText(/Question on M1/)).toBeInTheDocument();
    expect(within(group).getByText("Answered")).toBeInTheDocument();
  });

  it("approves a plan through the chat endpoint", async () => {
    const user = userEvent.setup();
    server.respond("POST", "/chat", 201, {
      id: 1,
      ts: "2026-11-15T12:01:00Z",
      sender: "developer",
      kind: "plan_approval",
      text: "",
      request: null,
      about: null,
      delivery: "queued",
      delivery_note: null,
    });
    await openWith([
      item({ id: "p-1", kind: "plan_approval", task: null, title: "Plan is ready", actions: ["approve", "open_chat"] }),
    ]);

    await user.click(await screen.findByRole("button", { name: "Approve" }));
    await waitFor(() => {
      const posts = server.requests.filter((r) => r.method === "POST" && r.path === "/chat");
      expect(posts).toHaveLength(1);
      expect(JSON.parse(posts[0]!.body)).toEqual({ kind: "plan_approval", text: "", request: null });
    });
  });

  it("AC2: approving a hold from an attention card calls /tasks/:id/approve, updating card and bell without reload", async () => {
    const user = userEvent.setup();
    server.respond("POST", "/tasks/M2/approve", 200, { status: "ok" });
    await openWith([item({ id: "hold-1", kind: "held", task: "M2", title: "Hold on M2", actions: ["approve"] })]);

    expect(await screen.findByRole("button", { name: /Notifications, 1 need attention/ })).toBeInTheDocument();
    expect(screen.getByText("Hold on M2")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => {
      const posts = server.requests.filter((r) => r.path === "/tasks/M2/approve");
      expect(posts).toHaveLength(1);
    });
    await waitFor(() => expect(screen.queryByText("Hold on M2")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.getByRole("contentinfo")).toHaveTextContent("nothing needs you");
  });

  it("AC2: retrying from an attention card calls /dispatch, updating card and bell without reload", async () => {
    const user = userEvent.setup();
    server.respond("POST", "/dispatch", 200, { dispatched: ["M3"] });
    await openWith([
      item({ id: "fail-1", kind: "failed", task: "M3", title: "Verification failed on M3", actions: ["dispatch"] }),
    ]);

    expect(await screen.findByRole("button", { name: /Notifications, 1 need attention/ })).toBeInTheDocument();
    expect(screen.getByText("Verification failed on M3")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Retry" }));

    await waitFor(() => {
      const posts = server.requests.filter((r) => r.path === "/dispatch");
      expect(posts).toHaveLength(1);
      expect(JSON.parse(posts[0]!.body)).toEqual({ ids: ["M3"] });
    });
    await waitFor(() => expect(screen.queryByText("Verification failed on M3")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.getByRole("contentinfo")).toHaveTextContent("nothing needs you");
  });

  it("AC5: lists open and resolved items with search and status filters and clear empty state", async () => {
    const user = userEvent.setup();
    await openWith([
      item({ id: "q-1", title: "Question on M1", detail: "Use YAML or JSON?" }),
      item({ id: "h-1", kind: "held", task: "M2", title: "Milestone M2 held", detail: "Migration risk", actions: ["approve"] }),
    ]);

    // Search filter
    const searchInput = screen.getByPlaceholderText(/Search by milestone, agent or message/);
    await user.type(searchInput, "YAML");
    expect(screen.getByText("Question on M1")).toBeInTheDocument();
    expect(screen.queryByText("Milestone M2 held")).not.toBeInTheDocument();

    // Clear search
    await user.clear(searchInput);
    expect(screen.getByText("Question on M1")).toBeInTheDocument();
    expect(screen.getByText("Milestone M2 held")).toBeInTheDocument();

    // Empty state when search matches nothing
    await user.type(searchInput, "nonexistent query text");
    expect(screen.getByText("No notifications match these filters.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Reset filters" }));

    // Status filter: Open vs Resolved vs All
    await user.click(screen.getByRole("tab", { name: /Resolved/ }));
    expect(screen.getByText("No notifications match these filters.")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: /Open/ }));
    expect(screen.getByText("Question on M1")).toBeInTheDocument();
  });

  it("AC5: status tabs are walkable by keyboard with arrow keys and Home/End", async () => {
    const user = userEvent.setup();
    await openWith([
      item({ id: "q-1", title: "Question on M1" }),
    ]);

    const openTab = screen.getByRole("tab", { name: /Open/ });
    const resolvedTab = screen.getByRole("tab", { name: /Resolved/ });
    const allTab = screen.getByRole("tab", { name: /All/ });
    const panel = screen.getByRole("tabpanel");

    expect(openTab).toHaveAttribute("aria-selected", "true");
    expect(openTab).toHaveAttribute("tabindex", "0");
    expect(resolvedTab).toHaveAttribute("aria-selected", "false");
    expect(resolvedTab).toHaveAttribute("tabindex", "-1");
    expect(panel).toHaveAttribute("aria-labelledby", "status-tab-open");

    // Focus active tab and navigate with ArrowRight
    openTab.focus();
    await user.keyboard("{ArrowRight}");
    expect(resolvedTab).toHaveFocus();
    expect(resolvedTab).toHaveAttribute("aria-selected", "true");
    expect(panel).toHaveAttribute("aria-labelledby", "status-tab-resolved");

    // Navigate to All with ArrowRight
    await user.keyboard("{ArrowRight}");
    expect(allTab).toHaveFocus();
    expect(allTab).toHaveAttribute("aria-selected", "true");
    expect(panel).toHaveAttribute("aria-labelledby", "status-tab-all");

    // Navigate back to Open with Home
    await user.keyboard("{Home}");
    expect(openTab).toHaveFocus();
    expect(openTab).toHaveAttribute("aria-selected", "true");

    // Navigate to End (All)
    await user.keyboard("{End}");
    expect(allTab).toHaveFocus();
    expect(allTab).toHaveAttribute("aria-selected", "true");

    // Wrap around from Open with ArrowLeft
    await user.keyboard("{Home}{ArrowLeft}");
    expect(allTab).toHaveFocus();
  });
});
