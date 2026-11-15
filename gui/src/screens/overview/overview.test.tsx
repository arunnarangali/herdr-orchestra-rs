import { render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { events, serve, status } from "../../components/testFixtures";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { RunOverview, isBlocked } from "./RunOverview";

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
      const summary = screen.getByRole("region", { name: "Run summary" });
      expect(within(summary).getByText(label)).toHaveAttribute("data-run-state", control ?? runState);
      if (broken) expect(screen.getByRole("alert")).toHaveTextContent("the check fails");
    });
  }
});

describe("RunOverview: content", () => {
  it("shows branch, event manager, event counts, progress, workers and groups", () => {
    const s = status({
      branch: "orch/desktop",
      event_manager: { running: true, holder: "orch autopilot (pid 4)" },
      events: { unread: 2, open: 3 },
      tasks: [
        { id: "M1", title: "Done one", state: "merged", deps: [], worker: null, tier: null, reason: "" },
        { id: "M2", title: "Held one", state: "held", deps: ["M1"], worker: null, tier: null, reason: "touches the schema" },
        { id: "M3", title: "Failed one", state: "failed", deps: [], worker: null, tier: null, reason: "check failed" },
        { id: "M4", title: "Waiting one", state: "needs-answer", deps: [], worker: null, tier: null, reason: "which port?" },
        { id: "M5", title: "Blocked one", state: "pending", deps: ["M2"], worker: null, tier: null, reason: "" },
      ],
      progress: [{ state: "merged", count: 1 }, { state: "held", count: 1 }, { state: "failed", count: 1 }, { state: "needs-answer", count: 1 }, { state: "pending", count: 1 }],
    });
    show(<RunOverview status={s} events={events()} />);
    expect(screen.getByText("orch/desktop")).toBeInTheDocument();
    expect(screen.getByText(/running \(orch autopilot \(pid 4\)\)/)).toBeInTheDocument();
    expect(screen.getByText("2 unread, 3 open")).toBeInTheDocument();
    expect(screen.getByText("Progress: 1 of 5 merged")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Milestones merged" })).toHaveAttribute("value", "1");

    const workers = screen.getByRole("region", { name: "Running workers" });
    expect(within(workers).getByText("rs-claude-1")).toBeInTheDocument();
    expect(within(workers).queryByText("rs-codex-1")).not.toBeInTheDocument(); // no milestone: not running

    const group = (name: string) => screen.getByRole("region", { name });
    expect(within(group("Held")).getByText("touches the schema")).toBeInTheDocument();
    expect(within(group("Failed")).getByText("check failed")).toBeInTheDocument();
    expect(within(group("Waiting for an answer")).getByText("which port?")).toBeInTheDocument();
    expect(within(group("Blocked")).getByText(/Blocked one/)).toBeInTheDocument();
    expect(within(group("Completed")).getByText(/Done one/)).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Recent events" })).toHaveTextContent("M1 needs an answer");
  });

  it("empty parts say so", () => {
    show(<RunOverview status={status({ workers: [], tasks: [], progress: [] })} events={null} />);
    expect(screen.getByText("No worker is on a milestone right now.")).toBeInTheDocument();
    expect(screen.getByText("Nothing is held.")).toBeInTheDocument();
    expect(screen.getByText("No events yet.")).toBeInTheDocument();
  });

  it("isBlocked: pending with an unmerged dependency only", () => {
    const s = status();
    const m3 = s.tasks[2]!;
    expect(isBlocked(m3, s.tasks)).toBe(true);
    expect(isBlocked(s.tasks[1]!, s.tasks)).toBe(false);
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
    expect(await screen.findByText("Progress: 1 of 3 merged")).toBeInTheDocument();
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    await waitFor(() => expect(screen.getByText("Live updates: on")).toBeInTheDocument());

    serve(server, { status: status({ progress: [{ state: "merged", count: 2 }, { state: "pending", count: 1 }] }) });
    server.emit(1, "task-state", { id: "M2", to: "merged" });
    expect(await screen.findByText("Progress: 2 of 3 merged")).toBeInTheDocument();
  });

  it("shows the paused control state from the stream", async () => {
    serve(server);
    renderApp({ route: "/overview", initial: server });
    await screen.findByText("Progress: 1 of 3 merged");
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    server.emit(5, "run-state", { run_control: "paused" });
    await waitFor(() => expect(screen.getByText("Paused")).toHaveAttribute("data-run-state", "paused"));
  });

  it("has an empty state when the project has no run", async () => {
    server.respond("GET", "/status", 404, errorBody("no_run", "no run"));
    server.respond("GET", "/events", 404, errorBody("no_run", "no run"));
    renderApp({ route: "/overview", initial: server });
    expect(await screen.findByText("No run in this project yet")).toBeInTheDocument();
  });

  it("has an error state with what to do", async () => {
    server.respond("GET", "/status", 500, errorBody("internal", "internal error"));
    renderApp({ route: "/overview", initial: server });
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("internal error");
    expect(within(alert).getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });
});
