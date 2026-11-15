import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { samples } from "../../api/contract";
import type { AgentKindInfo } from "../../api/types";
import { startFakeServer, type FakeServer } from "../../test/fakeServer";
import { fakeShell, renderApp } from "../../test/render";
import type { Shell } from "../../app/shell";
import { setGitCheckForTests } from "./git";

const kind = (o: Partial<AgentKindInfo>): AgentKindInfo =>
  Object.assign<AgentKindInfo, Partial<AgentKindInfo>>({ ...samples.agents.agents[0]!, login: { state: "authenticated", reason: "" } }, o);

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  server.respond("GET", "/agents", 200, {
    agents: [kind({ kind: "claude", display_name: "Claude Code" }), kind({ kind: "codex", display_name: "Codex", installed: false, launch: "codex" })],
    team: [],
  });
});
afterEach(async () => {
  setGitCheckForTests(undefined);
  await server.close();
});

const missingShell = () =>
  fakeShell({
    findOrch: async () => Promise.reject(new Error("orch was not found: set ORCH_BIN to its full path or put it on your PATH")),
    findHerdr: async () => Promise.reject(new Error("herdr was not found: set HERDR_BIN or put it on your PATH")),
  });
const show = (o: Parameters<typeof renderApp>[0] = {}) => renderApp({ route: "/onboarding", ...o });
const step = () => screen.getByRole("article");
const indicator = () => screen.getByRole("navigation", { name: "Setup steps" });

describe("first launch", () => {
  it("opens the flow with a step indicator and all five steps", () => {
    show({ shell: fakeShell() });
    const items = within(indicator()).getAllByRole("button");
    expect(items.map((b) => b.textContent)).toEqual([
      expect.stringContaining("1. orch"),
      expect.stringContaining("2. Herdr"),
      expect.stringContaining("3. Agents"),
      expect.stringContaining("4. Project"),
      expect.stringContaining("5. Start"),
    ]);
    expect(items[0]).toHaveAttribute("aria-current", "step");
    expect(screen.getByText("Step 1 of 5")).toBeInTheDocument();
  });

  it("is where the app lands when no project is chosen", async () => {
    renderApp({ route: "/overview", shell: fakeShell() });
    expect(await screen.findByRole("heading", { name: "Getting started" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Setup steps" })).toBeInTheDocument();
  });
});

describe("everything missing", () => {
  it("each step says what is missing and what to do, with commands to copy, and none blocks the next", async () => {
    const user = userEvent.setup();
    show({ shell: missingShell() });

    await screen.findByText(/orch was not found/);
    expect(step()).toHaveTextContent("set ORCH_BIN");
    await user.click(screen.getByRole("button", { name: "Copy orch --version" }));
    expect(await navigator.clipboard.readText()).toBe("orch --version");
    expect(await screen.findByText("Copied")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText(/herdr was not found/)).toBeInTheDocument();
    expect(step()).toHaveTextContent("never installs it");
    expect(screen.getByRole("button", { name: "Copy herdr --version" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(step()).toHaveTextContent("Choose a project folder");

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(step()).toHaveTextContent("No project is chosen yet");
    expect(screen.getByRole("button", { name: "Choose your git repository…" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    // the last step can be read although every step before it failed
    expect(step()).toHaveTextContent("Still open: orch, Herdr, Project");
    expect(step()).toHaveTextContent("describe the feature to the lead");
    expect(step()).toHaveTextContent("go");
    expect(screen.getByRole("button", { name: "Copy orch team up" })).toBeInTheDocument();
    expect(within(indicator()).getAllByText("needs attention").length).toBeGreaterThanOrEqual(2);
  });

  it("with no agent installed the agents step explains what to install", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/agents", 200, { agents: [kind({ kind: "claude", installed: false }), kind({ kind: "codex", installed: false })], team: [] });
    show({ initial: server });
    await user.click(screen.getByRole("button", { name: /3\. Agents/ }));
    expect(await screen.findByText(/0 of 2 agents found/)).toBeInTheDocument();
    expect(step()).toHaveTextContent("Install at least one coding-agent CLI");
  });

  it("without a project the agents step waits for one instead of failing", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell() });
    await user.click(screen.getByRole("button", { name: /3\. Agents/ }));
    expect(step()).toHaveTextContent("starts once a project is chosen");
  });
});

describe("everything present", () => {
  it("shows what was found at each step and walks to the end", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell({ findOrch: async () => "/usr/bin/orch", findHerdr: async () => "/usr/bin/herdr" }), initial: server });
    expect(await screen.findByText("orch found at /usr/bin/orch.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Herdr found at /usr/bin/herdr.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText(/1 of 2 agents found: Claude Code\. Missing: Codex \(optional\)/)).toBeInTheDocument();
    const missing = screen.getByRole("list", { name: "Missing agents" });
    expect(missing).toHaveTextContent("Codex is not installed");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText(/Project: \/home\/me\/project/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(step()).toHaveTextContent("Everything checked is in place.");
    expect(screen.getByRole("button", { name: "Copy orch team claude" })).toBeInTheDocument();
  });

  it("steps can be checked again", async () => {
    const user = userEvent.setup();
    let found = false;
    const shell = fakeShell({ findOrch: async () => (found ? "/usr/bin/orch" : Promise.reject(new Error("orch was not found"))) });
    show({ shell });
    expect(await screen.findByText("orch was not found")).toBeInTheDocument();
    found = true;
    await user.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByText("orch found at /usr/bin/orch.")).toBeInTheDocument();
  });
});

describe("choosing a project", () => {
  const goToProject = async (user: ReturnType<typeof userEvent.setup>) => user.click(screen.getByRole("button", { name: /4\. Project/ }));

  it("a folder that is not a git repository is refused with what to do, and no server is started", async () => {
    const user = userEvent.setup();
    const startServer = vi.fn<Shell["startServer"]>();
    setGitCheckForTests(async () => ({ kind: "no", reason: "/work/plain is not a git repository" }));
    show({ shell: fakeShell({ chooseProjectFolder: async () => "/work/plain", startServer }) });
    await goToProject(user);
    await user.click(screen.getByRole("button", { name: "Choose your git repository…" }));
    expect(await screen.findByText(/\/work\/plain is not a git repository/)).toBeInTheDocument();
    expect(step()).toHaveTextContent("git init");
    expect(startServer).not.toHaveBeenCalled();
    expect(within(indicator()).getByRole("button", { name: /4\. Project/ })).toHaveTextContent("needs attention");
  });

  it("a git repository starts the server, connects, and the agents step then runs", async () => {
    const user = userEvent.setup();
    const startServer = vi.fn<Shell["startServer"]>().mockResolvedValue({ url: server.url, token: server.token });
    setGitCheckForTests(async () => ({ kind: "yes" }));
    show({ shell: fakeShell({ chooseProjectFolder: async () => "/work/repo", startServer }) });
    await goToProject(user);
    await user.click(screen.getByRole("button", { name: "Choose your git repository…" }));
    expect(await screen.findByText(/Project: \/home\/me\/project/)).toBeInTheDocument();
    expect(startServer).toHaveBeenCalledWith("/work/repo");
    await user.click(screen.getByRole("button", { name: /3\. Agents/ }));
    expect(await screen.findByText(/1 of 2 agents found/)).toBeInTheDocument();
  });

  it("when the shell cannot say whether it is a repository the step says so instead of guessing", async () => {
    const user = userEvent.setup();
    setGitCheckForTests(async () => ({ kind: "unknown" }));
    show({ shell: fakeShell({ chooseProjectFolder: async () => "/work/repo", startServer: async () => ({ url: server.url, token: server.token }) }) });
    await goToProject(user);
    await user.click(screen.getByRole("button", { name: "Choose your git repository…" }));
    expect(await screen.findByText(/could not confirm it is a git repository/)).toBeInTheDocument();
  });

  it("cancelling the folder dialog changes nothing; a failing start shows its message", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell({ chooseProjectFolder: async () => null }) });
    await goToProject(user);
    await user.click(screen.getByRole("button", { name: "Choose your git repository…" }));
    expect(step()).toHaveTextContent("No project is chosen yet");
  });
});

describe("skipping, finishing and reopening", () => {
  it("skipping leaves the flow and it can be started again", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell() });
    await user.click(screen.getByRole("button", { name: "Skip setup" }));
    expect(screen.getByText("Setup was skipped.")).toBeInTheDocument();
    expect(window.localStorage.getItem("orch.onboarding")).toBe("skipped");
    expect(screen.queryByRole("navigation", { name: "Setup steps" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Start the guided setup" }));
    expect(screen.getByRole("navigation", { name: "Setup steps" })).toBeInTheDocument();
  });

  it("a skipped flow is not shown again on the next launch, and is reachable from the Help menu", async () => {
    window.localStorage.setItem("orch.onboarding", "skipped");
    const user = userEvent.setup();
    show({ shell: fakeShell() });
    expect(screen.getByText("Setup was skipped.")).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Setup steps" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Help" }));
    await user.click(screen.getByRole("menuitem", { name: "Getting started" }));
    expect(screen.getByRole("heading", { name: "Getting started" })).toBeInTheDocument();
  });

  it("finishing is remembered: a completed first run is not shown again", async () => {
    const user = userEvent.setup();
    const first = show({ shell: fakeShell() });
    await user.click(screen.getByRole("button", { name: /5\. Start/ }));
    await user.click(screen.getByRole("button", { name: "Finish setup" }));
    expect(screen.getByText("Setup is complete.")).toBeInTheDocument();
    expect(window.localStorage.getItem("orch.onboarding")).toBe("done");
    first.unmount();
    show({ shell: fakeShell() });
    expect(screen.getByText("Setup is complete.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Run the checks again" }));
    expect(screen.getByRole("navigation", { name: "Setup steps" })).toBeInTheDocument();
  });
});

describe("a browser", () => {
  it("says it cannot look for programs and gives commands instead", async () => {
    const user = userEvent.setup();
    show({ shell: null });
    expect(step()).toHaveTextContent("A browser cannot look for programs");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(step()).toHaveTextContent("herdr --version");
  });

  it("knows orch is installed when a server answers", async () => {
    show({ shell: null, initial: server });
    await waitFor(() => expect(step()).toHaveTextContent("A running orch 5.0.0 answered"));
  });

  it("is keyboard usable: steps are buttons, focus moves to the step heading", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell() });
    await user.tab();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: /2\. Herdr/ })).toHaveFocus();
  });
});
