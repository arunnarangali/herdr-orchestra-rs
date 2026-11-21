import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { finishedReport, finishedTasks, parallelPlan, runningReport, runningTasks, task } from "../workflows/fixtures";
import { estimateMinutes, fmtSeconds } from "./index";

describe("formatting", () => {
  it("reads seconds as minutes and hours", () => {
    expect(fmtSeconds(59)).toBe("1m");
    expect(fmtSeconds(1800)).toBe("30m");
    expect(fmtSeconds(3600)).toBe("1h 0m");
    expect(fmtSeconds(null)).toBe("—");
  });

  it("reads the plan's estimates as minutes", () => {
    expect(estimateMinutes("60m")).toBe(60);
    expect(estimateMinutes("1h 30m")).toBe(90);
    expect(estimateMinutes("none")).toBe(null);
    expect(estimateMinutes(null)).toBe(null);
  });
});

describe("Reports screen", () => {
  let server: FakeServer;
  beforeEach(async () => {
    server = await startFakeServer();
  });
  afterEach(() => server.close());

  const show = (report: unknown, plan = parallelPlan(), tasks: unknown = finishedTasks()) => {
    server.respond("GET", "/report", 200, report);
    server.respond("GET", "/plan", 200, plan);
    server.respond("GET", "/tasks", 200, Array.isArray(tasks) ? { tasks } : tasks);
    return renderApp({ route: "/reports", initial: server });
  };

  it("shows the finished run's numbers, including time against estimate", async () => {
    show(finishedReport());
    const metrics = await screen.findByRole("region", { name: "First-Pass Rate" });
    expect(metrics).toHaveTextContent("50%");
    expect(screen.getByText(/Branch: orch\/desktop/)).toBeInTheDocument();
    expect(screen.getByText("1h 0m of about 3h 0m planned")).toBeInTheDocument();
    expect(screen.getByText(/2 merged of 3/)).toBeInTheDocument();
    const wasted = screen.getByRole("region", { name: "Wasted Work" });
    expect(wasted).toHaveTextContent("1 lost attempt");
    expect(wasted).toHaveTextContent("1 fix round, 0 conflict rounds, 0 extra attempts");
    // the run did not merge everything: partial, not ALL_DONE
    expect(screen.getByText(/Run Report:/)).toHaveTextContent(/PARTIAL|FAILED/);
  });

  it("the per-model table shows the tiers and the escalations", async () => {
    show(
      finishedReport(),
      parallelPlan(),
      finishedTasks().tasks.map((t) => (t.id === "M2" ? { ...t, escalated: true } : t)),
    );
    await screen.findByRole("columnheader", { name: "Escalations" });
    const table = screen.getByRole("table", { name: /What each model did/ });
    expect(table).toHaveTextContent("claude-sonnet-5-5");
    expect(table).toHaveTextContent("codex-5");
    const rows = table.querySelectorAll("tbody tr");
    expect(rows).toHaveLength(2);
    expect(rows[1]).toHaveTextContent("1"); // one escalation for codex-5
  });

  it("each milestone row shows the planned tier against the model that did the work", async () => {
    show(finishedReport());
    await screen.findByRole("cell", { name: /medium → small/ });
    const table = screen.getByRole("table", { name: /Each milestone's result/ });
    const m1 = table.querySelector('tr[data-milestone="M1"]');
    const m2 = table.querySelector('tr[data-milestone="M2"]');
    const m3 = table.querySelector('tr[data-milestone="M3"]');
    expect(m1).toHaveTextContent("merged");
    expect(m1).toHaveTextContent("claude-sonnet-5-5");
    expect(m1).toHaveTextContent("20m"); // 1200s of work
    expect(m2).toHaveTextContent("codex-5");
    expect(m3).toHaveTextContent("failed");
    expect(m3).toHaveTextContent("claude-sonnet-5-5");
    expect(m3).toHaveTextContent("medium → small (below plan)");
  });

  it("a milestone's details show its checks, review findings and files changed", async () => {
    const user = userEvent.setup();
    show(finishedReport());
    server.respond("GET", "/tasks/M1", 200, samples.taskDetail);
    await screen.findByRole("table", { name: /Each milestone's result/ });
    expect(screen.queryByText("Verification (check)")).not.toBeInTheDocument();
    await user.click(screen.getAllByRole("button", { name: "Details" })[0]!);
    expect(await screen.findByText(/42 tests pass/)).toBeInTheDocument();
    expect(screen.getAllByText(/the token is logged/).length).toBeGreaterThan(1); // blocker and suggestion
    expect(screen.getByText("gui/package.json")).toBeInTheDocument(); // files changed
    expect(screen.getByText(/outside the milestone's likely files/)).toHaveTextContent("docs/x.md");
    await user.click(screen.getAllByRole("button", { name: "Hide details" })[0]!);
    expect(screen.queryByText("Verification (check)")).not.toBeInTheDocument();
  });

  it("a detail that fails to load says so instead of hanging", async () => {
    const user = userEvent.setup();
    show(finishedReport());
    server.respond("GET", "/tasks/M1", 500, errorBody("internal", "boom"));
    await screen.findByRole("table", { name: /Each milestone's result/ });
    await user.click(screen.getAllByRole("button", { name: "Details" })[0]!);
    expect(await screen.findByText(/could not load M1/)).toBeInTheDocument();
  });

  it("lists files outside the likely files and the reviewer's suggestions, and creates a follow-up fix from one", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/tasks/M1", 200, samples.taskDetail);
    server.respond("POST", "/tasks/M1/fix", 200, { ...samples.action, message: "M1-fix-1 created" });
    show(finishedReport());
    const scope = await screen.findByRole("region", { name: /Scope Warnings/ });
    await waitFor(() => expect(scope).toHaveTextContent("docs/x.md"));
    const sugg = screen.getByRole("region", { name: "Unapplied Reviewer Suggestions" });
    const button = within(sugg).getAllByRole("button", { name: /Create a follow-up fix/ })[0]!;
    await user.click(button);
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Create fix" }));
    await waitFor(() => expect(server.requests.some((r) => r.method === "POST" && r.path === "/tasks/M1/fix")).toBe(true));
    expect(await within(sugg).findByText("M1-fix-1 created")).toBeInTheDocument();
    expect(button).toBeDisabled();
  });

  it("filters the milestones to rework, and the actions the API lacks say why", async () => {
    const user = userEvent.setup();
    show(finishedReport());
    const table = await screen.findByRole("table", { name: /Each milestone's result/ });
    const ids = () => [...table.querySelectorAll("tr[data-milestone]")].map((r) => r.getAttribute("data-milestone"));
    expect(ids()).toEqual(["M1", "M2", "M3"]);
    // rework: more than one try, a fix round, a second review or a conflict
    await user.click(screen.getByRole("button", { name: "Rework" }));
    expect(ids()).toEqual(["M2", "M3"]);
    await user.click(screen.getByRole("button", { name: "Escalated" }));
    expect(table).toHaveTextContent("No milestone matches this filter.");
    await user.click(screen.getByRole("button", { name: "All" }));
    expect(ids()).toHaveLength(3);
    expect(screen.getByRole("button", { name: /View summary\.md/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Finish & Create PR/ })).toHaveAttribute("title", expect.stringContaining("orch finish"));
    expect(screen.getByRole("button", { name: "Export JSON" })).toBeEnabled();
  });

  it("a run in progress says so and still shows the partial numbers", async () => {
    show(runningReport(), parallelPlan(), runningTasks());
    expect(await screen.findByText("Run in progress — the numbers below are partial.")).toBeInTheDocument();
    expect(screen.getByText("1 merged of 3")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "First-Pass Rate" })).toHaveTextContent("100%");
    expect(screen.getByText("Elapsed so far")).toBeInTheDocument();
    expect(screen.getByText(/Run Report:/)).toHaveTextContent("RUNNING");
  });

  it("no run yet: an error that says how to create one", async () => {
    server.respond("GET", "/report", 404, errorBody("no_run", "no run exists"));
    server.respond("GET", "/plan", 404, errorBody("no_run", "no run exists"));
    server.respond("GET", "/tasks", 404, errorBody("no_run", "no run exists"));
    renderApp({ route: "/reports", initial: server });
    expect(await screen.findByText("This project has no run yet")).toBeInTheDocument();
    expect(screen.getByText("No report yet")).toBeInTheDocument();
    expect(screen.getByText(/orch sync/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument(); // a normal state, not an error
  });

  it("a run with no milestones shows the empty state", async () => {
    show({ ...finishedReport(), tasks: [], models: [] });
    expect(await screen.findByText(/This run has no milestones/)).toBeInTheDocument();
    expect(screen.getByText(/orch sync/)).toBeInTheDocument();
  });

  it("a server that is down shows an error with a retry", async () => {
    const url = await (async () => {
      const s = await startFakeServer();
      const u = s.url;
      await s.close();
      return u;
    })();
    renderApp({ route: "/reports", initial: { url, token: server.token } });
    expect(await screen.findByText("Cannot reach the server")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });
});

describe("Reports contract sample", () => {
  it("the report sample's rows render against the plan sample", async () => {
    const server = await startFakeServer();
    try {
      server.respond("GET", "/report", 200, samples.report);
      server.respond("GET", "/plan", 200, samples.plan);
      server.respond("GET", "/tasks", 200, samples.tasks);
      renderApp({ route: "/reports", initial: server });
      expect(await screen.findByRole("region", { name: "First-Pass Rate" })).toHaveTextContent("50%");
      expect(screen.getByRole("table", { name: /What each model did/ })).toHaveTextContent(
        "claude-sonnet-5-5",
      );
    } finally {
      await server.close();
    }
  });
});

describe("escalation fixture", () => {
  it("keeps the escalated flag through the task fixture", () => {
    expect(task({ id: "M9", escalated: true }).escalated).toBe(true);
  });
});
