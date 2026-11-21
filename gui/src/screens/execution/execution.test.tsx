import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import { detail, events, serve, task } from "../../components/testFixtures";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { disabledReason } from "./RunControls";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
});
afterEach(() => server.close());

const open = (id = "M1") => renderApp({ route: `/execution?task=${id}`, initial: server });
const count = (method: string, path: string) => server.requests.filter((r) => r.method === method && r.path === path).length;
const post = (path: string) => server.requests.filter((r) => r.method === "POST" && r.path === path);
const ok = (message = "done") => ({ ...samples.action, message });

describe("milestone list and detail", () => {
  it("lists every milestone with state, worker, model, attempts and rounds", async () => {
    serve(server, {
      tasks: [
        task({ id: "M1", title: "Foundation", state: "merged", worker: null, last_worker: "rs-claude-1", attempt: 2, fix_rounds: 1, review_rounds: 2, conflict_rounds: 0 }),
        task({ id: "M2", title: "Overview", state: "coding", worker: "rs-codex-1", model: "gpt-x", attempt: 1, fix_rounds: 0, review_rounds: 0, conflict_rounds: 0 }),
      ],
    });
    renderApp({ route: "/execution", initial: server });
    const table = await screen.findByRole("table", { name: "Milestones" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(3);
    expect(rows[1]).toHaveTextContent("merged");
    expect(rows[1]).toHaveTextContent("rs-claude-1");
    expect(rows[1]).toHaveTextContent("2");
    expect(rows[1]).toHaveTextContent("1/2/0");
    expect(rows[2]).toHaveTextContent("gpt-x");
  });

  it("opens a milestone: history, checks, reviews, result, files and logs, as plain text", async () => {
    const t = task();
    const d = detail(t, {
      files_outside_likely: ["docs/x.md"],
      results: [{ ...samples.taskDetail.results[0]!, commit: "<script>alert(1)</script>", questions: ["<b>which</b> port?"] }],
    });
    serve(server, { tasks: [t], detail: d, logs: ["12:00 <img src=x onerror=alert(1)>"] });
    renderApp({ route: "/execution", initial: server });
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: /M1 Foundation/ }));

    expect(await screen.findByRole("heading", { name: "Transition history" })).toBeInTheDocument();
    expect(screen.getByText("dispatched")).toBeInTheDocument();
    expect(screen.getByText(/in the worktree/)).toBeInTheDocument();
    const review = screen.getByRole("article", { name: "Review round 1" });
    expect(within(review).getByText("not met")).toBeInTheDocument();
    expect(within(review).getByText("missing a test")).toBeInTheDocument();
    expect(within(review).getByText("Blockers (1)")).toBeInTheDocument();
    expect(within(review).getByText("Suggestions (1)")).toBeInTheDocument();
    expect(screen.getByText("docs/x.md")).toBeInTheDocument();
    expect(await screen.findByLabelText("Log lines")).toHaveTextContent("<img src=x onerror=alert(1)>");
    // text, never markup
    expect(screen.getByText(/<script>alert\(1\)<\/script>/)).toBeInTheDocument();
    expect(screen.getByText("<b>which</b> port?")).toBeInTheDocument();
    expect(document.querySelector("script, img, b")).toBeNull();
  });
});

describe("a milestone waiting for the developer", () => {
  it("marks it in the list and the panel", async () => {
    serve(server, { tasks: [task({ state: "needs-answer", reason: "which port should it use?" })] });
    open();
    const row = (await screen.findAllByText("Needs your answer"))[0]!.closest("tr")!;
    expect(row).toHaveClass("exec-row--needs");
    expect(row).toHaveTextContent("which port should it use?");
    expect(await screen.findByRole("note")).toHaveTextContent("which port should it use?");
  });

  it("answer: empty text is refused before sending; a real answer is sent and re-read", async () => {
    serve(server, { tasks: [task({ state: "needs-answer", reason: "port?" })] });
    server.respond("POST", "/tasks/M1/answer", 200, ok("answered M1"));
    const user = userEvent.setup();
    open();
    await user.click(await screen.findByRole("button", { name: "Send answer" }));
    expect(screen.getByRole("alert")).toHaveTextContent("empty answer is not sent");
    expect(post("/tasks/M1/answer")).toHaveLength(0);

    await user.type(screen.getByLabelText("Your answer to M1"), "use 8080");
    const before = count("GET", "/tasks");
    await user.click(screen.getByRole("button", { name: "Send answer" }));
    expect(await screen.findByText("answered M1")).toBeInTheDocument();
    expect(JSON.parse(post("/tasks/M1/answer")[0]!.body)).toEqual({ text: "use 8080" });
    await waitFor(() => expect(count("GET", "/tasks")).toBeGreaterThan(before));
    expect(screen.getByLabelText("Your answer to M1")).toHaveValue("");
  });

  it("held: approve asks first, showing the reasons, and only then calls the API", async () => {
    serve(server, { tasks: [task({ state: "held", reason: "touches the migrations folder" })] });
    server.respond("POST", "/tasks/M1/approve", 200, ok("approved M1"));
    const user = userEvent.setup();
    open();
    await user.click(await screen.findByRole("button", { name: "Approve…" }));
    const dialog = screen.getByRole("alertdialog", { name: "Approve M1?" });
    expect(dialog).toHaveTextContent("touches the migrations folder");
    expect(post("/tasks/M1/approve")).toHaveLength(0);

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(post("/tasks/M1/approve")).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "Approve…" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Approve" }));
    expect(await screen.findByText("approved M1")).toBeInTheDocument();
    expect(post("/tasks/M1/approve")).toHaveLength(1);
  });

  it("nothing approves on its own: a held milestone stays unapproved however long it sits", async () => {
    serve(server, { tasks: [task({ state: "held", reason: "risky" })] });
    open();
    await screen.findByRole("button", { name: "Approve…" });
    server.emit(1, "task-state", { id: "M1", to: "held" });
    server.emit(2, "lead-event", { id: 1, ts: 1, kind: "held", task: "M1", text: "M1 held" });
    await new Promise((r) => setTimeout(r, 700));
    expect(post("/tasks/M1/approve")).toHaveLength(0);
  });

  it("failed: shows why and offers retry; dispatch sends the milestone id", async () => {
    serve(server, { tasks: [task({ state: "failed", reason: "check failed twice" })] });
    server.respond("POST", "/dispatch", 200, ok("dispatched M1"));
    const user = userEvent.setup();
    open();
    expect((await screen.findAllByText("check failed twice")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Retry M1" }));
    expect(await screen.findByText("dispatched M1")).toBeInTheDocument();
    expect(JSON.parse(post("/dispatch")[0]!.body)).toEqual({ ids: ["M1"] });
  });

  it("fix: empty is refused; text is sent", async () => {
    serve(server, { tasks: [task({ state: "merged" })] });
    server.respond("POST", "/tasks/M1/fix", 200, ok("added M7"));
    const user = userEvent.setup();
    open();
    await user.click(await screen.findByRole("button", { name: "Add a follow-up fix" }));
    expect(screen.getByRole("alert")).toHaveTextContent("empty fix is not sent");
    expect(post("/tasks/M1/fix")).toHaveLength(0);
    await user.type(screen.getByLabelText(/Fix: what should change/), "rename the flag");
    await user.click(screen.getByRole("button", { name: "Add a follow-up fix" }));
    expect(await screen.findByText("added M7")).toBeInTheDocument();
    expect(JSON.parse(post("/tasks/M1/fix")[0]!.body)).toEqual({ text: "rename the flag" });
  });

  it("escalate and set status confirm first", async () => {
    serve(server, { tasks: [task({ state: "coding" })] });
    server.respond("POST", "/tasks/M1/escalate", 200, ok("escalated"));
    server.respond("POST", "/tasks/M1/status", 200, ok("M1 is now paused"));
    const user = userEvent.setup();
    open();
    await user.click(await screen.findByRole("button", { name: "Escalate…" }));
    expect(post("/tasks/M1/escalate")).toHaveLength(0);
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Escalate" }));
    expect(await screen.findByText("escalated")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Set status"), "skipped");
    await user.type(screen.getByPlaceholderText("Reason (optional)"), "not needed");
    await user.click(screen.getByRole("button", { name: "Set status…" }));
    expect(post("/tasks/M1/status")).toHaveLength(0);
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Set to skipped" }));
    expect(await screen.findByText("M1 is now paused")).toBeInTheDocument();
    expect(JSON.parse(post("/tasks/M1/status")[0]!.body)).toEqual({ status: "skipped", reason: "not needed" });
  });

  it("a refused action shows the API's message and leaves the screen as it was", async () => {
    serve(server, { tasks: [task({ state: "held", reason: "risky" })] });
    server.respond("POST", "/tasks/M1/approve", 409, errorBody("conflict", "M1 is not held any more: it is merged"));
    const user = userEvent.setup();
    open();
    await user.click(await screen.findByRole("button", { name: "Approve…" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Approve" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("M1 is not held any more: it is merged");

    const reads = count("GET", "/tasks") + count("GET", "/tasks/M1") + count("GET", "/events");
    await new Promise((r) => setTimeout(r, 400));
    expect(count("GET", "/tasks") + count("GET", "/tasks/M1") + count("GET", "/events")).toBe(reads);
    expect(screen.getByRole("button", { name: "Approve…" })).toBeInTheDocument();
    expect(screen.getAllByText("held").length).toBeGreaterThan(0);
  });
});

describe("run controls", () => {
  it("disabledReason covers each state", () => {
    expect(disabledReason("pause", "paused")).toMatch(/already paused/);
    expect(disabledReason("resume", "running")).toMatch(/not paused/);
    expect(disabledReason("stop", "stopped")).toMatch(/already stopped/);
    expect(disabledReason("pause", "stopping")).toMatch(/stopping/);
    expect(disabledReason("pause", null)).toBeNull();
    expect(disabledReason("resume", "paused")).toBeNull();
  });

  it("pause asks first and then shows the new control state", async () => {
    serve(server);
    server.respond("POST", "/run/pause", 200, { ...samples.runControl, run_control: "paused", message: "no new work starts" });
    const user = userEvent.setup();
    renderApp({ route: "/execution", initial: server });
    await screen.findByRole("region", { name: "Run controls" });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    await user.click(screen.getByRole("button", { name: "Pause" }));
    expect(post("/run/pause")).toHaveLength(0);
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Pause" }));
    expect(await screen.findByText("no new work starts")).toBeInTheDocument();
    const controls = screen.getByRole("region", { name: "Run controls" });
    await waitFor(() => expect(within(controls).getByText("Paused")).toBeInTheDocument());
    expect(within(controls).getByRole("button", { name: "Pause" })).toBeDisabled();
    expect(within(controls).getByText("The run is already paused.")).toBeInTheDocument();
    expect(within(controls).getByRole("button", { name: "Resume" })).toBeEnabled();
  });

  it("stop says what it does before it is sent", async () => {
    serve(server);
    server.respond("POST", "/run/stop", 200, { ...samples.runControl, run_control: "stopping", message: "stopping" });
    const user = userEvent.setup();
    renderApp({ route: "/execution", initial: server });
    await user.click(await screen.findByRole("button", { name: "Stop" }));
    const dialog = screen.getByRole("alertdialog", { name: "Stop the run?" });
    expect(dialog).toHaveTextContent("running coder's work is saved");
    expect(post("/run/stop")).toHaveLength(0);
    await user.click(within(dialog).getByRole("button", { name: "Stop the run" }));
    expect(await screen.findByText("stopping", { selector: "p" })).toBeInTheDocument();
  });

  it("follows the stream's run-state and disables what does not apply", async () => {
    serve(server);
    renderApp({ route: "/execution", initial: server });
    await screen.findByRole("region", { name: "Run controls" });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    server.emit(3, "run-state", { run_control: "running" });
    await waitFor(() => expect(screen.getByRole("button", { name: "Resume" })).toBeDisabled());
    expect(screen.getByText("The run is not paused.")).toBeInTheDocument();
  });
});

describe("live behaviour", () => {
  it("an event arriving updates the screen", async () => {
    serve(server, { tasks: [task({ state: "coding" })] });
    renderApp({ route: "/execution", initial: server });
    const table = await screen.findByRole("table", { name: "Milestones" });
    expect(within(table).getByText("coding")).toBeInTheDocument();
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    serve(server, { tasks: [task({ state: "needs-answer", reason: "which port?" })] });
    server.emit(1, "task-state", { id: "M1", to: "needs-answer" });
    expect(await within(table).findByText("needs-answer")).toBeInTheDocument();
    expect(within(table).getByText("Needs your answer")).toBeInTheDocument();
  });

  it("new questions, holds and the end of the run show an in-app notice", async () => {
    serve(server);
    renderApp({ route: "/execution", initial: server });
    await screen.findByRole("region", { name: "Run controls" });
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    server.emit(1, "lead-event", { id: 1, ts: 1, kind: "needs-answer", task: "M2", text: "which port?" });
    server.emit(2, "lead-event", { id: 2, ts: 2, kind: "held", task: "M3", text: "touches migrations" });
    server.emit(3, "lead-event", { id: 3, ts: 3, kind: "all-done", text: "every milestone merged" });
    server.emit(4, "lead-event", { id: 4, ts: 4, kind: "merged", task: "M1", text: "not a notice" });
    const list = await screen.findByRole("status", { name: "Notices" });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(3));
    expect(list).toHaveTextContent("Question M2: which port?");
    expect(list).toHaveTextContent("Held M3");
    expect(list).toHaveTextContent("Run finished: every milestone merged");
    const user = userEvent.setup();
    await user.click(within(list).getAllByRole("button", { name: "Dismiss notice" })[0]!);
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
  });

  it("shows the lead's queue and dispatches a ready milestone", async () => {
    serve(server, { events: events({ ready: [{ task: "M2", title: "Overview", tier: "small", workers: ["rs-claude-1"] }] }) });
    server.respond("POST", "/dispatch", 200, ok("dispatched M2"));
    const user = userEvent.setup();
    renderApp({ route: "/execution", initial: server });
    const queue = await screen.findByRole("region", { name: "Lead's queue" });
    expect(queue).toHaveTextContent("M1 needs an answer");
    await user.click(within(queue).getByRole("button", { name: "Dispatch M2" }));
    expect(await within(queue).findByText("dispatched M2")).toBeInTheDocument();
    expect(JSON.parse(post("/dispatch")[0]!.body)).toEqual({ ids: ["M2"] });
  });

  it("the queue updates live", async () => {
    serve(server, { events: events({ new: [], still_open: [], ready: [] }) });
    renderApp({ route: "/execution", initial: server });
    const queue = await screen.findByRole("region", { name: "Lead's queue" });
    expect(queue).toHaveTextContent("Nothing new.");
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    serve(server);
    server.emit(1, "lead-event", { id: 7, ts: 1, kind: "needs-answer", task: "M1", text: "q" });
    await waitFor(() => expect(queue).toHaveTextContent("M1 needs an answer"));
  });

  it("the connection dropping shows the reconnecting state", async () => {
    serve(server);
    renderApp({ route: "/execution", initial: server });
    expect(await screen.findByText("Live updates: on")).toBeInTheDocument();
    const { port, token } = server;
    await server.close();
    expect(await screen.findByText(/connection lost, reconnecting/)).toHaveAttribute("data-stream", "reconnecting");
    const back = await startFakeServer({ port, token });
    server = back;
    serve(back);
    expect(await screen.findByText("Live updates: on")).toBeInTheDocument();
  });
});

describe("states", () => {
  it("no run: an empty state, not an error", async () => {
    server.respond("GET", "/tasks", 404, errorBody("no_run", "no run"));
    server.respond("GET", "/events", 404, errorBody("no_run", "no run"));
    renderApp({ route: "/execution", initial: server });
    expect(await screen.findByText("No run in this project yet")).toBeInTheDocument();
  });

  it("a run without milestones is empty", async () => {
    serve(server, { tasks: [] });
    renderApp({ route: "/execution", initial: server });
    expect(await screen.findByText("The run has no milestones.")).toBeInTheDocument();
  });

  it("an expired token is a clear error", async () => {
    server.respond("GET", "/tasks", 401, errorBody("unauthorized", "wrong token"));
    renderApp({ route: "/execution", initial: server });
    expect(await screen.findByText("The server refused the token")).toBeInTheDocument();
  });

  it("is keyboard usable: a row opens with Enter and the actions are reachable by Tab", async () => {
    serve(server, { tasks: [task({ state: "needs-answer", reason: "q" })] });
    renderApp({ route: "/execution", initial: server });
    const user = userEvent.setup();
    const rowButton = await screen.findByRole("button", { name: /M1 Foundation/ });
    rowButton.focus();
    await user.keyboard("{Enter}");
    expect(rowButton).toHaveAttribute("aria-expanded", "true");
    expect(await screen.findByLabelText("Your answer to M1")).toBeInTheDocument();
  });
});

describe("AC4: Execution Monitor controls, progress, elapsed and budget", () => {
  it("shows state badge, progress bar, elapsed vs budget, and pause/resume/stop with stop confirmation", async () => {
    serve(server, {
      tasks: [
        task({ id: "M1", state: "merged" }),
        task({ id: "M2", state: "coding", worker: "rs-claude-1" }),
      ],
    });
    server.respond("POST", "/run/pause", 200, { ...samples.runControl, run_control: "paused", message: "Run paused." });
    server.respond("POST", "/run/resume", 200, { ...samples.runControl, run_control: "running", message: "Run resumed." });
    server.respond("POST", "/run/stop", 200, { ...samples.runControl, run_control: "stopped", message: "Run stopped." });

    const user = userEvent.setup();
    renderApp({ route: "/execution", initial: server });

    // State badge
    const badge = await screen.findByText("Live", { selector: "#mon-live" });
    expect(badge).toBeInTheDocument();

    // Progress bar and counts
    const progBar = screen.getByRole("progressbar", { name: "Run progress" });
    expect(progBar).toBeInTheDocument();
    expect(screen.getByText(/50% \(1\/2 Merged, 1 Coding\)/)).toBeInTheDocument();

    // Elapsed vs budget
    expect(screen.getByText(/Budget:/)).toBeInTheDocument();
    expect(document.getElementById("elapsed")).toBeInTheDocument();
    expect(document.getElementById("budget")).toBeInTheDocument();

    // Pause
    const pauseBtn = screen.getByRole("button", { name: "Pause" });
    await user.click(pauseBtn);
    const pauseDialog = screen.getByRole("alertdialog", { name: "Pause the run?" });
    await user.click(within(pauseDialog).getByRole("button", { name: "Pause" }));
    expect(post("/run/pause")).toHaveLength(1);

    // Resume
    server.emit(1, "run-state", { run_control: "paused" });
    const resumeBtn = await screen.findByRole("button", { name: "Resume" });
    await user.click(resumeBtn);
    const resumeDialog = screen.getByRole("alertdialog", { name: "Resume the run?" });
    await user.click(within(resumeDialog).getByRole("button", { name: "Resume" }));
    expect(post("/run/resume")).toHaveLength(1);

    // Stop with confirmation
    server.emit(2, "run-state", { run_control: "running" });
    const stopBtn = await screen.findByRole("button", { name: "Stop" });
    await user.click(stopBtn);
    const stopDialog = screen.getByRole("alertdialog", { name: "Stop the run?" });
    expect(stopDialog).toHaveTextContent("running coder's work is saved");
    expect(post("/run/stop")).toHaveLength(0);
    await user.click(within(stopDialog).getByRole("button", { name: "Stop the run" }));
    expect(post("/run/stop")).toHaveLength(1);
  });
});

describe("AC5: Agent cards, output tabs and timeline without stealing focus", () => {
  it("renders agent cards, output tabs navigable by keyboard, timeline ack and open actions, preserving focus during live updates", async () => {
    serve(server, {
      tasks: [
        task({ id: "M1", title: "Foundation", state: "merged", worker: "rs-claude-1", model: "claude-sonnet-5-5" }),
        task({ id: "M2", title: "Review", state: "coding", worker: "rs-codex-1", model: "gpt-5-codex" }),
      ],
      events: events({
        new: [
          {
            id: 101,
            ts: 1700000200,
            kind: "VerificationPassed",
            task: "M1",
            task_title: "Foundation",
            text: "check passed",
            read: false,
            open: false,
            help: "",
          },
        ],
      }),
    });

    const user = userEvent.setup();
    renderApp({ route: "/execution", initial: server });

    // Agent cards
    const agentClaude = await screen.findByRole("article", { name: "rs-claude-1" });
    expect(agentClaude).toBeInTheDocument();
    expect(within(agentClaude).getByText(/M1: Foundation/)).toBeInTheDocument();
    expect(within(agentClaude).queryByText(/Worktree/)).toBeNull();

    // Output tabs & keyboard navigation
    const tabsList = screen.getByRole("tablist", { name: "Output sources" });
    const autopilotTab = within(tabsList).getByRole("tab", { name: "autopilot.log" });
    expect(autopilotTab).toHaveAttribute("aria-selected", "true");

    // Click tab focuses and selects it
    const claudeTab = within(tabsList).getByRole("tab", { name: "rs-claude-1.log" });
    await user.click(claudeTab);
    expect(claudeTab).toHaveAttribute("aria-selected", "true");

    // Arrow keys move between tabs
    claudeTab.focus();
    await user.keyboard("{ArrowLeft}");
    expect(autopilotTab).toHaveAttribute("aria-selected", "true");

    // Event timeline: Ack has no API call, so it is disabled with a reason and never claims success
    const queue = screen.getByRole("region", { name: "Lead's queue" });
    const ackBtn = within(queue).getByRole("button", { name: "Acknowledge VerificationPassed" });
    expect(ackBtn).toBeDisabled();
    expect(within(queue).getByText(/Acknowledge and Replay are not available yet/)).toBeInTheDocument();
    expect(within(queue).queryByText("Acked")).toBeNull();
    // Open goes to the milestone
    await user.click(within(queue).getAllByRole("button", { name: "Open M1" })[0]!);
    expect(await screen.findByRole("button", { name: /M1 Foundation/, expanded: true })).toBeInTheDocument();

    // Verify live stream update does NOT move keyboard focus
    claudeTab.focus();
    expect(document.activeElement).toBe(claudeTab);
    server.respond(
      "GET",
      "/events",
      200,
      events({
        new: [
          {
            id: 102,
            ts: 1700000300,
            kind: "TaskReady",
            task: "M3",
            task_title: "Reports",
            text: "ready for review",
            read: false,
            open: false,
            help: "",
          },
        ],
      }),
    );
    server.emit(10, "lead-event", { id: 102, ts: 1700000300, kind: "TaskReady", task: "M3", text: "ready for review" });
    await waitFor(() => expect(screen.getByText("ready for review")).toBeInTheDocument());
    expect(document.activeElement).toBe(claudeTab);
  });
});

describe("output tabs use real output", () => {
  it("autopilot shows /logs; an agent tab shows that agent's output and keeps focus on a live update", async () => {
    serve(server, { tasks: [task({ id: "M2", state: "coding", worker: "rs-claude-1" })], logs: ["12:00 dispatched M2"] });
    server.respond("GET", "/agents/rs-claude-1/output", 200, { agent: "rs-claude-1", lines: 2, text: "compiling\ntests ok", ts: "now" });
    const user = userEvent.setup();
    renderApp({ route: "/execution", initial: server });
    const log = await screen.findByRole("tabpanel");
    expect(await within(log).findByText(/dispatched M2/)).toBeInTheDocument();
    const tab = screen.getByRole("tab", { name: "rs-claude-1.log" });
    await user.click(tab);
    expect(await within(screen.getByRole("tabpanel")).findByText(/tests ok/)).toBeInTheDocument();
    tab.focus();
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    server.emit(5, "task-state", { id: "M2", to: "verifying" });
    await waitFor(() => expect(count("GET", "/agents/rs-claude-1/output")).toBeGreaterThan(1));
    expect(document.activeElement).toBe(tab);
  });
});
