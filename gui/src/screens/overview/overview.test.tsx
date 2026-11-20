import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { samples } from "../../api/contract";
import type { TaggedItem } from "../../components/attentionStore";
import { events, serve, status } from "../../components/testFixtures";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { RunOverview, progressOf, recentEvents } from "./RunOverview";

const show = (ui: React.ReactElement) => render(<MemoryRouter>{ui}</MemoryRouter>);

describe("RunOverview: every run state", () => {
  const cases: [string, string, string | null][] = [
    ["running", "Running", null],
    ["running", "Paused", "paused"],
    ["idle", "Idle", null],
    ["all_done", "All done", null],
    ["stopped", "Stopped", null],
    ["broken_branch", "Broken branch", null],
  ];
  for (const [runState, label, control] of cases) {
    it(`${runState}${control ? ` with control ${control}` : ""} reads "${label}"`, () => {
      const broken = runState === "broken_branch" ? { reason: "the check fails", since: 1 } : null;
      show(<RunOverview status={status({ run_state: runState, broken_branch: broken })} events={null} control={control} />);
      const summary = screen.getByRole("region", { name: "Lead State" });
      expect(within(summary).getByText(label)).toHaveAttribute("data-run-state", control ?? runState);
      if (broken) expect(screen.getByRole("alert")).toHaveTextContent("the check fails");
    });
  }
});

describe("RunOverview: content", () => {
  const busy = status({
    branch: "orch/desktop",
    base: "main",
    event_manager: { running: true, holder: "orch autopilot (pid 4)" },
    events: { unread: 2, open: 3 },
    max_parallel: 4,
    tasks: [
      { id: "M1", title: "Done one", state: "merged", deps: [], worker: null, tier: null, reason: "" },
      { id: "M2", title: "Held one", state: "held", deps: ["M1"], worker: null, tier: null, reason: "touches the schema" },
      { id: "M3", title: "Failed one", state: "failed", deps: [], worker: null, tier: null, reason: "check failed" },
      { id: "M4", title: "Waiting one", state: "needs-answer", deps: [], worker: null, tier: null, reason: "which port?" },
      { id: "M5", title: "Blocked one", state: "pending", deps: ["M2"], worker: null, tier: null, reason: "" },
    ],
    progress: [{ state: "merged", count: 1 }, { state: "held", count: 1 }, { state: "failed", count: 1 }, { state: "needs-answer", count: 1 }, { state: "pending", count: 1 }],
  });

  it("shows progress, lead state, workers, health, git and what needs the developer", () => {
    show(<RunOverview status={busy} events={events()} plan={samples.plan} health={samples.health} />);
    const region = (name: string) => screen.getByRole("region", { name });

    const progress = region("Overall Progress");
    expect(within(progress).getByRole("progressbar", { name: "Overall progress" })).toHaveAttribute("aria-valuenow", "20");
    expect(within(progress).getByRole("list", { name: "Milestones by state" })).toHaveTextContent("1 Merged");
    expect(within(progress).getByRole("list", { name: "Milestones by state" })).toHaveTextContent("2 Held");
    expect(within(progress).getByRole("list", { name: "Milestones by state" })).toHaveTextContent("1 Failed");
    expect(progress).toHaveTextContent("1 of 5 merged");

    const lead = region("Lead State");
    expect(lead).toHaveTextContent("Lead Mode:");
    expect(lead).toHaveTextContent("2");
    expect(lead).toHaveTextContent("3 open");

    const workers = region("Active Workers");
    expect(workers).toHaveTextContent("1 / 4");
    expect(within(workers).getByText("rs-claude-1")).toBeInTheDocument();
    expect(within(workers).queryByText("rs-codex-1")).not.toBeInTheDocument(); // no milestone: not running

    const health = region("System Health");
    expect(health).toHaveTextContent("Healthy");
    expect(within(health).getByText(/running \(orch autopilot \(pid 4\)\)/)).toBeInTheDocument();

    const git = region("Active Orchestration & Git");
    expect(within(git).getByText("orch/desktop")).toBeInTheDocument();
    expect(git).toHaveTextContent("Base: main");
    // read from the plan, changed only through the plan: shown, but not a working control
    expect(within(git).getByRole("button", { name: /ON/ })).toBeDisabled();
    expect(within(git).getByRole("button", { name: /warn/ })).toHaveAttribute("title", expect.stringContaining("Not available yet"));

    const attention = region("Needs Attention (Action Required)");
    expect(attention).toHaveTextContent("3 items");
    expect(attention).toHaveTextContent("M2 Held");
    expect(within(attention).getByText(/touches the schema/)).toBeInTheDocument();
    expect(within(attention).getByText(/check failed/)).toBeInTheDocument();
    expect(within(attention).getByText(/which port\?/)).toBeInTheDocument();
    expect(within(attention).queryByText(/Blocked one/)).not.toBeInTheDocument();

    const ev = region("Recent Execution Events");
    expect(ev).toHaveTextContent("M1 needs an answer");
    expect(within(ev).getByRole("button", { name: /Acknowledge event 7/ })).toBeDisabled();
  });

  it("acts on attention items: Approve for a hold, Answer opens the item", async () => {
    const user = userEvent.setup();
    const item = (o: Partial<TaggedItem>): TaggedItem => ({
      id: "a", kind: "held", task: "M2", agent: null, title: "M2 held", detail: "touches the schema", prompt: null, since: "",
      actions: ["approve"], uid: "u-a", sourceKey: "", projectName: null, ...o,
    });
    const onApprove = vi.fn(async () => {});
    const onReview = vi.fn();
    const held = item({});
    const question = item({ id: "q", uid: "u-q", kind: "question", task: "M4", agent: "coder-2", detail: "which port?", actions: ["answer"] });
    show(<RunOverview status={busy} events={null} attention={{ items: [held, question], onApprove, onReview }} />);
    const attention = screen.getByRole("region", { name: "Needs Attention (Action Required)" });
    // M2 and M4 are covered by their items; M3 comes from the status
    expect(attention).toHaveTextContent("3 items");
    expect(attention).toHaveTextContent("coder-2 asks: which port?");
    await user.click(within(attention).getByRole("button", { name: /Approve/ }));
    expect(onApprove).toHaveBeenCalledWith(held);
    await user.click(within(attention).getByRole("button", { name: /Answer/ }));
    expect(onReview).toHaveBeenCalledWith(question);
  });

  it("filters the events to the unread ones", async () => {
    const user = userEvent.setup();
    const read = { ...samples.events.new[0]!, id: 3, read: true, text: "M0 merged", kind: "merged" };
    show(<RunOverview status={busy} events={events({ new: [samples.events.new[0]!, read], still_open: [] })} />);
    const ev = screen.getByRole("region", { name: "Recent Execution Events" });
    expect(ev).toHaveTextContent("M0 merged");
    await user.click(within(ev).getByRole("button", { name: "Unread (1)" }));
    expect(ev).not.toHaveTextContent("M0 merged");
    expect(ev).toHaveTextContent("M1 needs an answer");
  });

  it("empty parts say so", () => {
    show(<RunOverview status={status({ workers: [], tasks: [], progress: [] })} events={null} />);
    expect(screen.getByText("No worker is on a milestone right now.")).toBeInTheDocument();
    expect(screen.getByText("All clear")).toBeInTheDocument();
    expect(screen.getByText("No events yet.")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Overall progress" })).toHaveAttribute("aria-valuenow", "0");
  });

  it("progressOf groups the states and counts work in progress for half", () => {
    const p = progressOf(status({ progress: [{ state: "merged", count: 2 }, { state: "reviewing", count: 2 }, { state: "conflict", count: 1 }, { state: "pending", count: 3 }] }));
    expect(p.counts).toMatchObject({ merged: 2, coding: 2, failed: 1, pending: 3 });
    expect(p.percent).toBe(38);
  });

  it("recentEvents lists each event once, newest first", () => {
    const e = samples.events.new[0]!;
    const list = recentEvents(events({ new: [e, { ...e, id: 9, ts: e.ts + 5 }], still_open: [e] }));
    expect(list.map((x) => x.id)).toEqual([9, 7]);
  });
});

describe("OverviewScreen against a server", () => {
  let server: FakeServer;
  beforeEach(async () => {
    server = await startFakeServer();
  });
  afterEach(() => server.close());

  it("loads, then updates from the live stream without a refresh", async () => {
    serve(server);
    renderApp({ route: "/overview", initial: server });
    expect(await screen.findByText("1 of 3 merged")).toBeInTheDocument();
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    await waitFor(() => expect(screen.getByText("Live updates: on")).toBeInTheDocument());

    serve(server, { status: status({ progress: [{ state: "merged", count: 2 }, { state: "pending", count: 1 }] }) });
    server.emit(1, "task-state", { id: "M2", to: "merged" });
    expect(await screen.findByText("2 of 3 merged")).toBeInTheDocument();
  });

  it("shows the paused control state from the stream", async () => {
    serve(server);
    renderApp({ route: "/overview", initial: server });
    await screen.findByText("1 of 3 merged");
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    server.emit(5, "run-state", { run_control: "paused" });
    await waitFor(() => expect(screen.getByText("Paused")).toHaveAttribute("data-run-state", "paused"));
  });

  it("has an empty state when the project has no run", async () => {
    server.respond("GET", "/status", 404, errorBody("no_run", "no run"));
    server.respond("GET", "/events", 404, errorBody("no_run", "no run"));
    renderApp({ route: "/overview", initial: server });
    expect(await screen.findByText("No run in this project yet")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Plan a feature/ })).toHaveAttribute("href", "/chat");
  });

  it("an attention item of this project shows in Needs Attention; Approve calls the server", async () => {
    const user = userEvent.setup();
    serve(server);
    server.respond("GET", "/plan", 200, samples.plan);
    server.respond("POST", "/tasks/M2/approve", 200, samples.action);
    server.respond("GET", "/attention", 200, {
      items: [{ id: "h", kind: "held", task: "M2", agent: null, title: "M2 is held", detail: "touches the schema", prompt: null, since: "", actions: ["approve"] }],
    });
    renderApp({ route: "/overview", initial: server });
    const attention = await screen.findByRole("region", { name: "Needs Attention (Action Required)" });
    await within(attention).findByText(/touches the schema/);
    await user.click(within(attention).getByRole("button", { name: /Approve/ }));
    await waitFor(() => expect(server.requests.some((r) => r.method === "POST" && r.path === "/tasks/M2/approve")).toBe(true));
  });

  it("has an error state with what to do", async () => {
    server.respond("GET", "/status", 500, errorBody("internal", "internal error"));
    renderApp({ route: "/overview", initial: server });
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("internal error");
    expect(within(alert).getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });
});
