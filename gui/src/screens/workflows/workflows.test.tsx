import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { oneMilestonePlan, parallelPlan, task, twelvePlan } from "./fixtures";

describe("Workflows screen", () => {
  let server: FakeServer;
  beforeEach(async () => {
    server = await startFakeServer();
  });
  afterEach(() => server.close());

  const show = (plan: unknown, tasks: unknown = samples.tasks) => {
    server.respond("GET", "/plan", 200, plan);
    server.respond("GET", "/tasks", 200, Array.isArray(tasks) ? { tasks } : tasks);
    return renderApp({ route: "/workflows", initial: server });
  };

  it("shows loading, then the plan header settings and the feature breakdown", async () => {
    show(oneMilestonePlan());
    expect(screen.getAllByRole("status").some((el) => /Loading/.test(el.textContent ?? ""))).toBe(true);
    expect(await screen.findByRole("heading", { name: "Plan settings" })).toBeInTheDocument();
    expect(screen.getAllByText("orch/desktop").length).toBeGreaterThan(0);
    expect(screen.getAllByText("cargo build --workspace && npm test").length).toBeGreaterThan(0);
    expect(screen.getAllByText("worktree").length).toBeGreaterThan(0);
    expect(screen.getAllByText("2").length).toBeGreaterThan(0); // max parallel
    expect(screen.getAllByText("warn").length).toBeGreaterThan(0); // scope
    expect(screen.getAllByText("rs-claude-1").length).toBeGreaterThan(0); // the team
    expect(screen.getAllByText("rs-codex-1").length).toBeGreaterThan(0);
    expect(screen.getByText("1. a single slice")).toBeInTheDocument(); // feature breakdown
  });

  it("shows every milestone with its plan fields and current assignment", async () => {
    show(oneMilestonePlan(), {
      tasks: [
        {
          ...samples.tasks.tasks[0],
          id: "M1",
          state: "coding",
          worker: "rs-claude-1",
          model: "claude-sonnet-5-5",
        },
      ],
    });
    await screen.findByRole("heading", { name: "Milestones" });
    expect(screen.getAllByText("Goal of M1.").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Why M1 exists.").length).toBeGreaterThan(0);
    expect(screen.getAllByText("it is a distinct slice").length).toBeGreaterThan(0);
    expect(screen.getAllByText("medium").length).toBeGreaterThan(0);
    expect(screen.getAllByText(/coding, tool-use/).length).toBeGreaterThan(0);
    expect(screen.getAllByText("well specified and bounded").length).toBeGreaterThan(0);
    expect(screen.getAllByText("60m").length).toBeGreaterThan(0);
    expect(screen.getAllByText("front-end engineer").length).toBeGreaterThan(0);
    expect(screen.getAllByText("yes").length).toBeGreaterThan(0); // review
    expect(screen.getAllByText(/AC1/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/M1 works/).length).toBeGreaterThan(0);
    expect(screen.getAllByText("gui/src/screens/m1/").length).toBeGreaterThan(0);

    const card = screen.getAllByRole("listitem").find((li) => li.getAttribute("data-milestone") === "M1");
    expect(card).toBeDefined();
    expect(card).toHaveTextContent("coding"); // current state
    expect(card).toHaveTextContent("rs-claude-1"); // worker
    expect(card).toHaveTextContent("claude-sonnet-5-5"); // model
  });

  it("shows the open questions of a milestone that has them", async () => {
    show({
      ...oneMilestonePlan(),
      milestones: [
        { ...oneMilestonePlan().milestones[0]!, open_questions: "which port?", has_open_questions: true },
      ],
    });
    await screen.findByRole("heading", { name: "Milestones" });
    expect(screen.getAllByText("which port?").length).toBeGreaterThan(0);
  });

  it("draws a graph node per milestone and a readable list beside it", async () => {
    show(
      parallelPlan(),
      parallelPlan().milestones.map((m, i) => task({ id: m.id, title: m.title, state: i === 0 ? "merged" : "pending" })),
    );
    await screen.findByRole("group", { name: "Milestone dependency graph" });
    expect(screen.getByRole("button", { name: "M1 Shared base (merged)" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "M2 Left side (pending)" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "M3 Right side (pending)" })).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "Milestones as a list" });
    expect(list).toHaveTextContent("M2 Left side");
    expect(list).toHaveTextContent("after M1 — needs the shared base");
  });

  it("an edge's reason appears when focused, and again when the focus moves on", async () => {
    show(parallelPlan());
    await screen.findByRole("group", { name: "Milestone dependency graph" });
    const edge = screen.getByLabelText("M2 depends on M1, because needs the shared base");
    fireEvent.focus(edge);
    expect(screen.getByText("M2 depends on M1: needs the shared base")).toBeInTheDocument();
    fireEvent.blur(edge);
    expect(
      screen.getByText("Hover or focus a line to read why a milestone depends on another."),
    ).toBeInTheDocument();
  });

  it("selecting a node opens its detail and Escape closes it again", async () => {
    const user = userEvent.setup();
    show(parallelPlan());
    await screen.findByRole("group", { name: "Milestone dependency graph" });
    await user.click(screen.getByRole("button", { name: "M2 Left side (pending)" }));
    const dialog = screen.getByRole("dialog", { name: "M2: Left side" });
    expect(dialog).toHaveTextContent("Goal of M2.");
    expect(dialog).toHaveTextContent("Why M2 exists.");
    expect(dialog).toHaveTextContent("needs the shared base");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("stays readable with twelve milestones: twelve nodes, twelve cards", async () => {
    show(
      twelvePlan(),
      twelvePlan().milestones.map((m) => task({ id: m.id, title: m.title })),
    );
    await screen.findByRole("group", { name: "Milestone dependency graph" });
    expect(screen.getAllByRole("button", { name: /does its part \(pending\)/ })).toHaveLength(12);
    expect(screen.getAllByRole("listitem").filter((li) => li.hasAttribute("data-milestone"))).toHaveLength(12);
  });

  it("an empty plan says how to create one", async () => {
    show({ ...oneMilestonePlan(), milestones: [] });
    expect(
      await screen.findByText(/The plan has no milestones/),
    ).toBeInTheDocument();
    expect(screen.getByText(/orch sync/)).toBeInTheDocument();
  });

  it("no run yet: an error that says what to do", async () => {
    server.respond("GET", "/plan", 404, errorBody("no_run", "no run exists"));
    server.respond("GET", "/tasks", 404, errorBody("no_run", "no run exists"));
    renderApp({ route: "/workflows", initial: server });
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("This project has no run yet");
    expect(alert).toHaveTextContent("orch sync");
  });

  it("a server that is down shows an error with a retry", async () => {
    const url = await (async () => {
      const s = await startFakeServer();
      const u = s.url;
      await s.close();
      return u;
    })();
    renderApp({ route: "/workflows", initial: { url, token: server.token } });
    expect(await screen.findByText("Cannot reach the server")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("a refused token says so", async () => {
    server.respond("GET", "/plan", 401, errorBody("unauthorized", "wrong token"));
    server.respond("GET", "/tasks", 401, errorBody("unauthorized", "wrong token"));
    renderApp({ route: "/workflows", initial: server });
    expect(await screen.findByText("The server refused the token")).toBeInTheDocument();
  });
});

describe("Workflows contract sample", () => {
  it("the plan sample's milestone renders with its assignment", async () => {
    const server = await startFakeServer();
    try {
      server.respond("GET", "/plan", 200, samples.plan);
      server.respond("GET", "/tasks", 200, samples.tasks);
      renderApp({ route: "/workflows", initial: server });
      expect(await screen.findByText("Desktop app")).toBeInTheDocument();
      expect(screen.getAllByText("an app that starts").length).toBeGreaterThan(0);
    } finally {
      await server.close();
    }
  });
});
