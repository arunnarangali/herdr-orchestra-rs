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
    const table = await screen.findByRole("table", { name: "Agent kinds" });
    const rows = within(table).getAllByRole("row");
    expect(rows).toHaveLength(4);
    expect(rows[1]).toHaveTextContent("Claude Code");
    expect(rows[1]).toHaveTextContent("installed");
    expect(rows[1]).toHaveTextContent("worker, lead");
    expect(rows[1]).toHaveTextContent("claude --x");
    expect(rows[2]).toHaveTextContent("missing");
    expect(within(rows[2]!).getByLabelText("Problems with codex")).toHaveTextContent("not installed");
    expect(within(rows[3]!).getByLabelText("Problems with amp")).toHaveTextContent("not logged in");
    expect(rows[3]).toHaveTextContent("outdated");
  });

  it("lists the run's team with availability when a run exists", async () => {
    serve();
    show();
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
});

describe("Models", () => {
  const open = async () => {
    const user = userEvent.setup();
    show();
    await user.click(await screen.findByRole("tab", { name: "Models" }));
    return user;
  };

  it("tabs work by keyboard", async () => {
    serve();
    const user = userEvent.setup();
    show();
    const agents = await screen.findByRole("tab", { name: "Agents" });
    agents.focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Models" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Models" })).toHaveFocus();
    expect(await screen.findByRole("region", { name: "Catalog" })).toBeInTheDocument();
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
    await open();
    const catalog = await screen.findByRole("region", { name: "Catalog" });
    const alpha = within(catalog).getByRole("heading", { name: /alpha/ }).closest("li")!;
    expect(alpha).toHaveTextContent("high");
    expect(alpha).toHaveTextContent("claude");
    expect(alpha).toHaveTextContent("open weights: no");
    expect(within(alpha).getByLabelText("Reasoning of alpha")).toHaveAttribute("value", "5");
    expect(within(alpha).getByText("Speed").parentElement).toHaveTextContent("not rated");
    const policy = await screen.findByRole("region", { name: "Policy" });
    expect(policy).toHaveTextContent("Kept out of runs (1)");
    expect(policy).toHaveTextContent("kept out");
    expect(within(policy).getByText("best")).toBeInTheDocument();
  });

  it("compares the tier a milestone asked for with the team that can serve it", async () => {
    serve({
      agents: { agents: samples.agents.agents, team: [{ ...samples.agents.team[0]!, tier: "small", name: "weak-1" }] },
    });
    await open();
    const region = await screen.findByRole("region", { name: "Plan tiers against the team" });
    await waitFor(() => expect(region).toHaveTextContent("M1 Foundation"));
    expect(region).toHaveTextContent("medium");
    await waitFor(() => expect(region).toHaveTextContent("nobody on the team can serve this tier"));
  });

  it("names the team members that can serve a tier", async () => {
    serve();
    await open();
    await waitFor(() =>
      expect(screen.getByRole("region", { name: "Plan tiers against the team" })).toHaveTextContent("rs-claude-1 (medium)"),
    );
  });

  it("without a run the comparison explains why it is empty", async () => {
    serve({ plan: false });
    await open();
    expect(await screen.findByText(/no plan to compare/)).toBeInTheDocument();
  });
});
