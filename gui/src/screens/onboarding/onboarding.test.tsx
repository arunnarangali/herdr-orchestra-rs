import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { samples } from "../../api/contract";
import type { AgentKindInfo } from "../../api/types";
import { startFakeServer, type FakeServer } from "../../test/fakeServer";
import { fakeShell, renderApp } from "../../test/render";
import type { HerdrInfo, Shell } from "../../app/shell";
import { setGitCheckForTests } from "./git";

const kind = (o: Partial<AgentKindInfo>): AgentKindInfo =>
  Object.assign<AgentKindInfo, Partial<AgentKindInfo>>({ ...samples.agents.agents[0]!, login: { state: "authenticated", reason: "" } }, o);

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  server.respond("GET", "/agents", 200, {
    agents: [
      kind({ kind: "claude", display_name: "Claude Code", skills: { status: "current", problems: [] } }),
      kind({ kind: "codex", display_name: "Codex", installed: false, launch: "codex" }),
    ],
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
    findHerdr: async () => Promise.reject(new Error("Herdr was not found on this computer. Install Herdr from https://herdr.dev to continue.")),
  });
const show = (o: Parameters<typeof renderApp>[0] = {}) => renderApp({ route: "/onboarding", ...o });
const step = () => screen.getByRole("article");
const indicator = () => screen.getByRole("navigation", { name: "Setup steps" });

describe("first launch", () => {
  it("opens the flow with a step indicator and all six steps", () => {
    show({ shell: fakeShell() });
    const items = within(indicator()).getAllByRole("button");
    expect(items.map((b) => b.textContent)).toEqual([
      expect.stringContaining("1. orch"),
      expect.stringContaining("2. Herdr"),
      expect.stringContaining("3. Agents"),
      expect.stringContaining("4. Skills"),
      expect.stringContaining("5. Project"),
      expect.stringContaining("6. Start"),
    ]);
    expect(items[0]).toHaveAttribute("aria-current", "step");
    expect(screen.getByText("Step 1 of 6")).toBeInTheDocument();
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
    expect(await screen.findByText(/Herdr was not found/i)).toBeInTheDocument();
    expect(step()).toHaveTextContent("herdr.dev");
    expect(screen.getByRole("button", { name: "Install Herdr" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(step()).toHaveTextContent("Choose a project folder");

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(step()).toHaveTextContent("The skills are checked by the orch server");

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(step()).toHaveTextContent("No project is chosen yet");
    expect(screen.getByRole("button", { name: "Choose your git repository…" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    // the last step can be read although every step before it failed
    expect(step()).toHaveTextContent("Still open: orch, Herdr, Project");
    expect(step()).toHaveTextContent("Herdr needs attention");
    expect(within(step()).getByRole("button", { name: "Install Herdr" })).toBeInTheDocument();
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
  it("skills that are outdated need action, with the command that installs them", async () => {
    server.respond("GET", "/agents", 200, {
      agents: [kind({ kind: "codex", display_name: "Codex", skills: { status: "missing", problems: ["~/.codex/skills"] } })],
      team: [],
    });
    const user = userEvent.setup();
    show({ shell: fakeShell(), initial: server });
    await user.click(screen.getByRole("button", { name: /4\. Skills/ }));
    expect(await screen.findByText(/Skills missing for Codex \(~\/\.codex\/skills\)/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy orch agents sync" })).toBeInTheDocument();
    expect(within(indicator()).getByRole("button", { name: /4\. Skills/ })).toHaveTextContent("needs attention");
  });

  it("finishing offers the way to Chat / Home", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell(), initial: server });
    await user.click(screen.getByRole("button", { name: /6\. Start/ }));
    await user.click(screen.getByRole("button", { name: "Finish setup" }));
    await user.click(screen.getByRole("link", { name: /Go to Chat \/ Home/ }));
    expect(await screen.findByRole("heading", { name: "Chat" })).toBeInTheDocument();
  });

  it("shows what was found at each step and walks to the end", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell({ findOrch: async () => "/usr/bin/orch", findHerdr: async () => "/usr/bin/herdr" }), initial: server });
    expect(await screen.findByText("orch found at /usr/bin/orch.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText(/Herdr \(\/usr\/bin\/herdr\) was already running/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText(/1 of 2 agents found: Claude Code\. Missing: Codex \(optional\)/)).toBeInTheDocument();
    const missing = screen.getByRole("list", { name: "Missing agents" });
    expect(missing).toHaveTextContent("Codex is not installed");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText(/orch skills are installed and current for Claude Code/)).toBeInTheDocument();
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
  const goToProject = async (user: ReturnType<typeof userEvent.setup>) => user.click(screen.getByRole("button", { name: /5\. Project/ }));

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
    expect(within(indicator()).getByRole("button", { name: /5\. Project/ })).toHaveTextContent("needs attention");
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

  it("a skipped flow is not shown again on the next launch, and is reachable from the sidebar", async () => {
    window.localStorage.setItem("orch.onboarding", "skipped");
    const user = userEvent.setup();
    show({ shell: fakeShell() });
    expect(screen.getByText("Setup was skipped.")).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Setup steps" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("link", { name: "Onboarding" }));
    expect(screen.getByRole("heading", { name: "Getting started" })).toBeInTheDocument();
  });

  it("finishing is remembered: a completed first run is not shown again", async () => {
    const user = userEvent.setup();
    const first = show({ shell: fakeShell() });
    await user.click(screen.getByRole("button", { name: /6\. Start/ }));
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

describe("layout and step cards", () => {
  it("renders equal-width step cards with title, status badge, one-line desc, and card click switches step", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell(), initial: server });

    const nav = screen.getByRole("navigation", { name: "Setup steps" });
    expect(nav).toHaveClass("ob__steps-nav");

    const list = within(nav).getByRole("list");
    expect(list).toHaveClass("steps");

    const listItems = within(list).getAllByRole("listitem");
    expect(listItems).toHaveLength(6);
    for (const li of listItems) {
      expect(li).toHaveClass("step");
      const card = li.querySelector(".step__card");
      expect(card).not.toBeNull();
      expect(card?.querySelector(".step__desc")).not.toBeNull();
    }

    // Step 1 is active initially
    expect(listItems[0]).toHaveClass("step--on");

    // Clicking anywhere on step 3's card switches to step 3
    const step3Card = listItems[2]!.querySelector(".step__card") as HTMLElement;
    await user.click(step3Card);
    expect(listItems[2]).toHaveClass("step--on");
    expect(screen.getByRole("heading", { name: /3\. Installed AI Coding Agents/ })).toBeInTheDocument();
  });
});

const herdrInfo = (o: Partial<HerdrInfo> = {}): HerdrInfo => ({
  state: "running",
  installed: true,
  path: "/home/dev/.local/bin/herdr",
  version: "0.9.5",
  running: true,
  managed: true,
  too_old: false,
  error: null,
  fix: null,
  ...o,
});
const MISSING = herdrInfo({
  state: "missing",
  installed: false,
  path: null,
  version: null,
  running: false,
  managed: false,
  error: "Herdr is not installed on this computer. Orchestra needs it to run the Lead and its agents: choose Install Herdr to install it from Herdr's official site (herdr.dev).",
  fix: "install",
});
const SCRIPT = "#!/bin/sh\nset -eu\necho 'herdr installer'\n";

describe("the app's Herdr (AC1, AC2, AC4, AC6)", () => {
  it("AC2: missing Herdr offers Install Herdr, shows the official script before anything runs, and runs that plan on Run it", async () => {
    const user = userEvent.setup();
    const prepareHerdrFix = vi.fn(async () => ({ action: "install" as const, command: "curl -fsSL https://herdr.dev/install.sh | sh", script: SCRIPT, token: "t1" }));
    const runHerdrFix = vi.fn(async () => herdrInfo());
    show({ shell: fakeShell({ getHerdrInfo: async () => MISSING, prepareHerdrFix, runHerdrFix }) });

    await user.click(screen.getByRole("button", { name: /2\. Herdr/ }));
    expect(await within(step()).findByText(/Herdr is not installed on this computer/)).toBeInTheDocument();
    expect(step()).not.toHaveTextContent("HERDR_BIN");
    await user.click(within(step()).getByRole("button", { name: "Install Herdr" }));
    expect(prepareHerdrFix).toHaveBeenCalledWith("install");
    expect(runHerdrFix).not.toHaveBeenCalled();
    const plan = await screen.findByRole("region", { name: "Install Herdr: what will run" });
    expect(within(plan).getByLabelText("Installer script")).toHaveTextContent("echo 'herdr installer'");

    await user.click(within(plan).getByRole("button", { name: "Run it" }));
    expect(runHerdrFix).toHaveBeenCalledWith("t1");
    // Herdr now runs: the step is done and asks for nothing
    expect(await within(step()).findByText(/the app started it and stops it when you close the app/)).toBeInTheDocument();
    expect(within(step()).queryByRole("button", { name: "Install Herdr" })).toBeNull();
  });

  it("AC2 + AC4: when the install cannot be done from the app, it opens herdr.dev and says what to do next", async () => {
    const user = userEvent.setup();
    const openHerdrUrl = vi.fn(async () => {});
    const runHerdrFix = vi.fn(async () => {
      throw new Error("The Herdr installer stopped with an error:\nerror: unsupported architecture: sparc");
    });
    show({
      shell: fakeShell({
        getHerdrInfo: async () => MISSING,
        prepareHerdrFix: async () => ({ action: "install", command: "curl -fsSL https://herdr.dev/install.sh | sh", script: SCRIPT, token: "t" }),
        runHerdrFix,
        openHerdrUrl,
      }),
    });
    await user.click(screen.getByRole("button", { name: /2\. Herdr/ }));
    await user.click(await within(step()).findByRole("button", { name: "Install Herdr" }));
    await user.click(await screen.findByRole("button", { name: "Run it" }));
    const note = await within(step()).findByText(/unsupported architecture: sparc/);
    expect(note).toHaveTextContent("https://herdr.dev is open in your browser");
    expect(note).toHaveTextContent("then choose Check again");
    expect(openHerdrUrl).toHaveBeenCalledTimes(1);
  });

  it("AC2 + AC4: on the Start step the same panel is offered, with the same plan shown and run", async () => {
    const user = userEvent.setup();
    const prepareHerdrFix = vi.fn(async () => ({ action: "update" as const, command: "/usr/bin/herdr update", script: null, token: "u1" }));
    const runHerdrFix = vi.fn(async () => herdrInfo());
    const old = herdrInfo({ state: "too_old", running: false, managed: false, too_old: true, version: "0.8.2", path: "/usr/bin/herdr", error: "Herdr 0.8.2 is too old: Orchestra needs Herdr 0.9.0 or newer. Choose Update Herdr to update it with Herdr's own updater.", fix: "update" });
    show({ shell: fakeShell({ getHerdrInfo: async () => old, prepareHerdrFix, runHerdrFix }) });

    await user.click(screen.getByRole("button", { name: /6\. Start/ }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Herdr needs attention");
    expect(alert).toHaveTextContent("0.8.2 is too old");
    expect(alert).not.toHaveTextContent("not installed");
    await user.click(within(alert).getByRole("button", { name: "Update Herdr" }));
    const plan = await within(alert).findByRole("region", { name: "Update Herdr: what will run" });
    // what is shown is the plan's own command, and that plan is what runs
    expect(plan).toHaveTextContent("/usr/bin/herdr update");
    expect(plan).not.toHaveTextContent("curl");
    await user.click(within(plan).getByRole("button", { name: "Run it" }));
    expect(runHerdrFix).toHaveBeenCalledWith("u1");
  });

  it("AC4: a Herdr that is installed but did not start says so, offers Check again, and no install", async () => {
    const user = userEvent.setup();
    let n = 0;
    const getHerdrInfo = vi.fn(async () =>
      n++ === 0
        ? herdrInfo({ state: "failed", running: false, managed: false, error: "Herdr is installed, but its background server did not start (it did not answer within 15s). Check again; if it keeps failing, open herdr.dev for help." })
        : herdrInfo(),
    );
    show({ shell: fakeShell({ getHerdrInfo }) });
    await user.click(screen.getByRole("button", { name: /2\. Herdr/ }));
    expect(await within(step()).findByText(/background server did not start/)).toBeInTheDocument();
    expect(within(step()).queryByRole("button", { name: "Install Herdr" })).toBeNull();
    await user.click(within(step()).getByRole("button", { name: "Check again" }));
    expect(await within(step()).findByText(/runs in the background/)).toBeInTheDocument();
  });

  it("AC6: a running Herdr is shown as present, managed by the app, with nothing to install", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell({ getHerdrInfo: async () => herdrInfo() }) });
    await user.click(screen.getByRole("button", { name: /2\. Herdr/ }));
    expect(await within(step()).findByText(/Herdr 0\.9\.5 \(\/home\/dev\/\.local\/bin\/herdr\) runs in the background/)).toBeInTheDocument();
    expect(step()).toHaveTextContent("Nothing to do");
    expect(within(step()).queryByRole("button", { name: "Install Herdr" })).toBeNull();
    expect(within(step()).queryByRole("button", { name: /Copy/ })).toBeNull();
    // a Herdr the app found running is said to be left alone
    await user.click(screen.getByRole("button", { name: /1\. orch/ }));
  });

  it("AC6: a Herdr found already running is used and left running", async () => {
    const user = userEvent.setup();
    show({ shell: fakeShell({ getHerdrInfo: async () => herdrInfo({ managed: false }) }) });
    await user.click(screen.getByRole("button", { name: /2\. Herdr/ }));
    expect(await within(step()).findByText(/was already running: the app uses it and leaves it running/)).toBeInTheDocument();
  });
});
