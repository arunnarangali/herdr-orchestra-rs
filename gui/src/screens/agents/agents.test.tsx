import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import type { AgentKindInfo, ModelInfo } from "../../api/types";
import { deadAddress, errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { agentProblems } from "./advice";

const kind = (o: Partial<AgentKindInfo> = {}): AgentKindInfo =>
  Object.assign<AgentKindInfo, Partial<AgentKindInfo>>({ ...samples.agents.agents[0]!, skills: { status: "current", problems: [] } }, o);
const model = (o: Partial<ModelInfo> = {}): ModelInfo =>
  Object.assign<ModelInfo, Partial<ModelInfo>>({ ...samples.models.models[0]!, denied: false, denied_by: null }, o);

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
});
afterEach(() => server.close());

function serve(o: { agents?: unknown; models?: unknown; plan?: boolean; status?: boolean } = {}) {
  server.respond("GET", "/agents", 200, o.agents ?? samples.agents);
  server.respond("GET", "/models", 200, o.models ?? samples.models);
  if (o.plan === false) {
    server.respond("GET", "/plan", 404, errorBody("no_run", "no run"));
    server.respond("GET", "/status", 404, errorBody("no_run", "no run"));
  } else {
    server.respond("GET", "/plan", 200, samples.plan);
    server.respond("GET", "/status", 200, samples.status);
  }
}
const show = () => renderApp({ route: "/agents", initial: server });
const openTab = async (name: RegExp) => {
  const user = userEvent.setup();
  await user.click(await screen.findByRole("tab", { name }));
  return user;
};

describe("agentProblems", () => {
  it("is empty for a healthy agent", () => {
    expect(agentProblems(kind({ login: { state: "authenticated", reason: "" } }))).toEqual([]);
  });
  it("explains missing, logged out, unknown login, skills and herdr in words", () => {
    expect(agentProblems(kind({ installed: false }))[0]).toMatch(/not installed.*orch doctor/);
    expect(agentProblems(kind({ login: { state: "not authenticated", reason: "no file" } }))[0]).toMatch(/not logged in/);
    expect(agentProblems(kind({ login: { state: "unknown", reason: "" } }))[0]).toMatch(/could not be read/);
    expect(agentProblems(kind({ skills: { status: "missing", problems: ["a.md"] } })).join(" ")).toMatch(/skills are missing.*a\.md/);
    expect(agentProblems(kind({ skills: { status: "outdated", problems: ["b.md"] } })).join(" ")).toMatch(/outdated/);
    expect(agentProblems(kind({ herdr_detect: false })).join(" ")).toMatch(/Herdr cannot tell/);
  });
});

describe("Agents", () => {
  it("lists every kind with installed or missing, roles, herdr, login, skills and launch", async () => {
    serve({
      agents: {
        agents: [
          kind({ kind: "claude", display_name: "Claude Code", roles: ["worker", "lead"], launch: "claude --x", login: { state: "authenticated", reason: "" } }),
          kind({ kind: "codex", display_name: null, installed: false, herdr_detect: false, launch: "codex" }),
          kind({ kind: "amp", skills: { status: "outdated", problems: ["~/.amp/x.md"] }, login: { state: "not authenticated", reason: "no token" } }),
        ],
        team: [],
      },
    });
    show();
    const list = await screen.findByRole("list", { name: "Agent kinds" });
    const cards = within(list).getAllByRole("listitem").filter((li) => li.parentElement === list);
    expect(cards).toHaveLength(3);
    expect(cards[0]).toHaveTextContent("Claude Code");
    expect(cards[0]).toHaveTextContent("Logged In");
    expect(cards[0]).toHaveTextContent("Can be: worker, lead");
    expect(cards[0]).toHaveTextContent("claude --x");
    expect(cards[1]).toHaveTextContent("Not installed");
    expect(within(cards[1]!).getByLabelText("Problems with codex")).toHaveTextContent("not installed");
    expect(within(cards[2]!).getByLabelText("Problems with amp")).toHaveTextContent("not logged in");
    expect(cards[2]).toHaveTextContent("Skills Outdated");
  });

  it("an agent card opens its details: launch, models it runs and what can be done", async () => {
    const user = userEvent.setup();
    serve();
    show();
    const list = await screen.findByRole("list", { name: "Agent kinds" });
    const name = samples.agents.agents[0]!.display_name ?? samples.agents.agents[0]!.kind;
    await user.click(within(list).getByRole("button", { name: `${name} details` }));
    const dialog = await screen.findByRole("dialog", { name });
    expect(dialog).toHaveTextContent(samples.agents.agents[0]!.launch);
    expect(within(dialog).getByRole("button", { name: /Bench agent/ })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows the roles the run's team gives each kind on its card", async () => {
    serve();
    show();
    const list = await screen.findByRole("list", { name: "Agent kinds" });
    const m = samples.agents.team[0]!;
    expect(list).toHaveTextContent(`(${m.model})`);
  });

  it("lists the run's team with availability when a run exists", async () => {
    serve();
    show();
    await openTab(/Team Builder/);
    const team = await screen.findByRole("table", { name: "Team" });
    const row = within(team).getByRole("row", { name: /rs-claude-1/ });
    expect(row).toHaveTextContent("claude-sonnet-5-5");
    expect(row).toHaveTextContent("medium");
    await waitFor(() => expect(row).toHaveTextContent("busy on M1"));
    expect(screen.getByText(/benches are not shown/)).toBeInTheDocument();
  });

  it("without a run there is no team, and no error", async () => {
    serve({ plan: false, agents: { agents: samples.agents.agents, team: [] } });
    show();
    await openTab(/Team Builder/);
    expect(await screen.findByText(/no team/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("a server that is down gets an error with what to do", async () => {
    const dead = await deadAddress();
    renderApp({ route: "/agents", initial: { url: dead, token: "t" } });
    const alert = await screen.findByText("Cannot reach the server");
    expect(alert).toBeInTheDocument();
  });

  it("an expired token reads as such", async () => {
    server.respond("GET", "/agents", 401, errorBody("unauthorized", "no"));
    server.respond("GET", "/models", 401, errorBody("unauthorized", "no"));
    show();
    expect(await screen.findByText("The server refused the token")).toBeInTheDocument();
  });

  it("lists lead, coders and reviewer with what each is working on", async () => {
    serve({
      agents: {
        agents: samples.agents.agents,
        team: [
          {
            name: "lead-agent",
            role: "lead",
            kind: "claude",
            model: "claude-sonnet-5-5",
            tier: "high",
            detect: true,
            working_on: { task: "M1", state: "planning" },
          },
          {
            name: "coder-1",
            role: "worker",
            kind: "claude",
            model: "claude-haiku-4-5",
            tier: "small",
            detect: true,
            working_on: { task: "M2", state: "coding" },
          },
          {
            name: "reviewer-1",
            role: "reviewer",
            kind: "claude",
            model: "claude-sonnet-5-5",
            tier: "medium",
            detect: true,
            working_on: { task: "M3", state: "reviewing" },
          },
        ],
      },
    });
    show();
    await openTab(/Team Builder/);
    const team = await screen.findByRole("table", { name: "Team" });
    const leadRow = within(team).getByRole("row", { name: /lead-agent/ });
    expect(leadRow).toHaveTextContent("lead");
    expect(leadRow).toHaveTextContent("busy on M1 (planning)");

    const coderRow = within(team).getByRole("row", { name: /coder-1/ });
    expect(coderRow).toHaveTextContent("worker");
    expect(coderRow).toHaveTextContent("busy on M2 (coding)");

    const reviewerRow = within(team).getByRole("row", { name: /reviewer-1/ });
    expect(reviewerRow).toHaveTextContent("reviewer");
    expect(reviewerRow).toHaveTextContent("busy on M3 (reviewing)");
  });

  it("updates live when stream events occur", async () => {
    serve();
    show();
    await openTab(/Team Builder/);
    const team = await screen.findByRole("table", { name: "Team" });
    await waitFor(() => expect(team).toBeInTheDocument());
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));

    // Update response for next fetch
    server.respond("GET", "/agents", 200, {
      agents: samples.agents.agents,
      team: [
        {
          ...samples.agents.team[0]!,
          working_on: { task: "M2", state: "verifying" },
        },
      ],
    });

    // Emit stream event
    server.emit(10, "task-state", { task: "M2", state: "verifying" });

    await waitFor(() => {
      expect(screen.getByRole("table", { name: "Team" })).toHaveTextContent("busy on M2 (verifying)");
    });
  });

  it("opening an agent shows its output, with lines selector, search, pause/resume and auto-scroll", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/agents/rs-claude-1/output", 200, {
      agent: "rs-claude-1",
      lines: 2,
      text: "Line 1: building project\nLine 2: tests passed cleanly",
      ts: "2026-11-15T12:00:00Z",
    });

    serve();
    show();
    await openTab(/Team Builder/);

    const team = await screen.findByRole("table", { name: "Team" });
    const viewBtn = within(team).getByRole("button", { name: "View output for rs-claude-1" });
    await user.click(viewBtn);

    // Modal dialog is opened
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Live Output: rs-claude-1" })).toBeInTheDocument();
    expect(screen.getByText(/Line 1: building project/)).toBeInTheDocument();
    expect(screen.getByText(/Line 2: tests passed cleanly/)).toBeInTheDocument();

    // Pause and Resume control
    const pauseBtn = screen.getByRole("button", { name: "Pause refresh" });
    expect(screen.getByText("Live")).toBeInTheDocument();
    await user.click(pauseBtn);
    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Resume refresh" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Resume refresh" }));
    expect(screen.getByText("Live")).toBeInTheDocument();

    // Choose lines
    const linesSelect = screen.getByLabelText("Lines:");
    server.respond("GET", "/agents/rs-claude-1/output", 200, {
      agent: "rs-claude-1",
      lines: 50,
      text: "50 lines of output...",
      ts: "2026-11-15T12:01:00Z",
    });
    await user.selectOptions(linesSelect, "50");
    await waitFor(() => {
      const outputReqs = server.requests.filter((r) => r.path === "/agents/rs-claude-1/output");
      expect(outputReqs.some((r) => r.query.get("lines") === "50")).toBe(true);
    });

    // Search inside output
    const searchInput = screen.getByPlaceholderText("Search in output…");
    await user.type(searchInput, "lines");
    expect(await screen.findByText("1 match found")).toBeInTheDocument();
    expect(screen.getByText("lines")).toHaveClass("ag-output-hit");

    // Close button
    await user.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  });
});

describe("toolbar", () => {
  it("Scan Agents reads the agents again; the CLI-only actions say why they are off", async () => {
    serve();
    show();
    await screen.findByRole("list", { name: "Agent kinds" });
    const before = server.requests.filter((r) => r.path === "/agents").length;
    await userEvent.setup().click(screen.getByRole("button", { name: /Scan Agents/ }));
    await waitFor(() => expect(server.requests.filter((r) => r.path === "/agents").length).toBe(before + 1));
    expect(screen.getByRole("button", { name: /Run Doctor/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Sync Skills/ })).toHaveAttribute("title", expect.stringContaining("Not available yet"));
  });
});

describe("Models", () => {
  const open = async () => {
    const user = userEvent.setup();
    show();
    await user.click(await screen.findByRole("tab", { name: /Model Catalog/ }));
    return user;
  };

  it("tabs work by keyboard", async () => {
    serve();
    const user = userEvent.setup();
    show();
    const agents = await screen.findByRole("tab", { name: /Installed Agents/ });
    agents.focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: /Model Catalog/ })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: /Model Catalog/ })).toHaveFocus();
    expect(await screen.findByRole("region", { name: "Deny Policy Patterns" })).toBeInTheDocument();
    await user.keyboard("{End}");
    expect(screen.getByRole("tab", { name: "Team Builder" })).toHaveFocus();
    await user.keyboard("{Home}");
    expect(screen.getByRole("tab", { name: /Installed Agents/ })).toHaveAttribute("aria-selected", "true");
  });

  it("shows tier, ratings as labelled bars, kinds, open weights and what the policy keeps out", async () => {
    serve({
      models: {
        ...samples.models,
        models: [
          model({ id: "alpha", tier: "high", kinds: ["claude"], open_weights: false, reasoning: 5, coding: 4, speed: null }),
          model({ id: "best", tier: null, denied: true, denied_by: "best", open_weights: true }),
        ],
        policy: { deny: ["best"], deny_why: "kept out" },
      },
    });
    const user = await open();
    const catalog = await screen.findByRole("region", { name: /Shipped Model Catalog/ });
    const alpha = within(catalog).getByRole("row", { name: /alpha/ });
    expect(alpha).toHaveTextContent("high");
    expect(alpha).toHaveTextContent("claude · Proprietary");
    expect(alpha).toHaveTextContent("Allowed");
    const best = within(catalog).getByRole("row", { name: /best/ });
    expect(best).toHaveTextContent("DENIED BY POLICY (best)");
    expect(best).toHaveTextContent("unrated");
    await user.click(within(alpha).getByRole("button", { name: "Ratings of alpha" }));
    expect(within(catalog).getByLabelText("Reasoning of alpha")).toHaveAttribute("value", "5");
    expect(within(catalog).getByText("Speed").parentElement).toHaveTextContent("not rated");
    const policy = await screen.findByRole("region", { name: "Deny Policy Patterns" });
    expect(policy).toHaveTextContent("1 model is kept out of runs: best");
    expect(policy).toHaveTextContent("kept out");
    expect(within(policy).getByRole("list", { name: "Deny patterns" })).toHaveTextContent("best");
    expect(within(policy).getByRole("button", { name: /Add to deny list/ })).toBeDisabled();
  });

  it("filters the catalog by text, tier and policy", async () => {
    serve({
      models: {
        ...samples.models,
        models: [
          model({ id: "opus-x", tier: "high", kinds: ["claude"] }),
          model({ id: "haiku-x", tier: "small", kinds: ["claude"] }),
          model({ id: "gpt-x", tier: "medium", kinds: ["codex"] }),
          model({ id: "astra-x", tier: "high", kinds: ["agy"], denied: true, denied_by: "*astra*" }),
        ],
      },
    });
    const user = await open();
    const catalog = await screen.findByRole("region", { name: /Shipped Model Catalog/ });
    const ids = () => within(catalog).queryAllByRole("rowheader").map((c) => c.textContent);
    expect(ids()).toEqual(["opus-x", "haiku-x", "gpt-x", "astra-x"]);
    await user.selectOptions(within(catalog).getByLabelText("Tier"), "high");
    expect(ids()).toEqual(["opus-x", "astra-x"]);
    await user.selectOptions(within(catalog).getByLabelText("Policy"), "allowed");
    expect(ids()).toEqual(["opus-x"]);
    await user.selectOptions(within(catalog).getByLabelText("Policy"), "denied");
    expect(ids()).toEqual(["astra-x"]);
    await user.selectOptions(within(catalog).getByLabelText("Tier"), "all");
    await user.selectOptions(within(catalog).getByLabelText("Policy"), "all");
    await user.type(within(catalog).getByLabelText("Search models"), "codex");
    expect(ids()).toEqual(["gpt-x"]);
    await user.type(within(catalog).getByLabelText("Search models"), "zzz");
    expect(within(catalog).getByText("No models match these filters.")).toBeInTheDocument();
  });

  it("compares the tier a milestone asked for with the team that can serve it", async () => {
    serve({
      agents: { agents: samples.agents.agents, team: [{ ...samples.agents.team[0]!, tier: "small", name: "weak-1" }] },
    });
    await open();
    await openTab(/Team Builder/);
    const region = await screen.findByRole("region", { name: "Tier Routing" });
    await waitFor(() => expect(region).toHaveTextContent("M1 Foundation"));
    expect(region).toHaveTextContent("medium");
    await waitFor(() => expect(region).toHaveTextContent("nobody on the team can serve this tier"));
  });

  it("names the team members that can serve a tier", async () => {
    serve();
    await open();
    await openTab(/Team Builder/);
    await waitFor(() => expect(screen.getByRole("region", { name: "Tier Routing" })).toHaveTextContent("rs-claude-1 (medium)"));
  });

  it("without a run the comparison explains why it is empty", async () => {
    serve({ plan: false });
    await open();
    await openTab(/Team Builder/);
    expect(await screen.findByText(/no plan to compare/)).toBeInTheDocument();
  });
});
