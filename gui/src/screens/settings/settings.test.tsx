import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { samples } from "../../api/contract";
import { deadAddress, errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { setBackendForTests, type AppBackend, type AppSettings } from "../projects/backend";
import type { AgentCli } from "../../api/types";

const settings = (o: Partial<AppSettings> = {}): AppSettings => ({ theme: "system", notifications: false, orch_path: "", ...o });
function backend(o: Partial<AppBackend> = {}): AppBackend {
  return {
    chooseFolder: async () => null,
    listProjects: async () => ({ projects: [], active: null, open: [], warning: null }),
    addProject: async () => ({ projects: [], active: null, open: [], warning: null }),
    removeProject: async () => ({ projects: [], active: null, open: [], warning: null }),
    switchProject: async () => {
      throw new Error("x");
    },
    openProject: async () => ({ url: "", token: "" }),
    retryProject: async () => ({ url: "", token: "" }),
    closeProject: async () => ({ projects: [], active: null, open: [], warning: null }),
    restoreProjects: async () => [],
    createProject: async () => {
      throw new Error("x");
    },
    getSettings: async () => ({ value: settings(), warning: null }),
    saveSettings: async (s) => s,
    findOrch: async () => "/usr/bin/orch",
    detectAgentClis: async () => [],
    getProjectSettings: async (path) => ({ leadAgent: null, folder: path, warning: null }),
    saveProjectSettings: async (_path, s) => ({ leadAgent: s.leadAgent, folder: s.folder, warning: null }),
    ...o,
  };
}

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  server.respond("GET", "/plan", 200, samples.plan);
  server.respond("GET", "/status", 200, samples.status);
});
afterEach(async () => {
  setBackendForTests(undefined);
  await server.close();
});
const show = () => renderApp({ route: "/settings", initial: server });

const tab = (name: string) => screen.getByRole("tab", { name });
const panel = () => screen.getByRole("tabpanel");
const row = (name: string | RegExp) => within(panel()).getByRole("group", { name });
const open = async (user: ReturnType<typeof userEvent.setup>, name: string) => {
  await user.click(await screen.findByRole("tab", { name }));
  return panel();
};

describe("sections", () => {
  it("has every section of the prototype, walkable by keyboard", async () => {
    setBackendForTests(null);
    const user = userEvent.setup();
    show();
    const list = await screen.findByRole("tablist", { name: "Settings sections" });
    expect(within(list).getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "General", "Project", "Herdr Runtime", "Agent Providers", "Agent Roles", "Models & Policy", "Lead Mode", "Scheduling",
      "Parallel Execution", "Git & Worktrees", "Verification", "Review", "Risk Hold & Scope", "Recovery & Retries",
      "Notifications", "Storage", "Advanced TOML",
    ]);
    tab("General").focus();
    await user.keyboard("{ArrowDown}");
    expect(tab("Project")).toHaveFocus();
    expect(tab("Project")).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowDown}");
    expect(tab("Herdr Runtime")).toHaveFocus();
    expect(tab("Herdr Runtime")).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{End}");
    expect(tab("Advanced TOML")).toHaveAttribute("aria-selected", "true");
  });

  it("an option the app cannot save is disabled with the reason", async () => {
    setBackendForTests(null);
    show();
    await screen.findByRole("tab", { name: "General" });
    const density = within(row("Interface Density")).getByRole("combobox");
    expect(density).toBeDisabled();
    expect(density).toHaveAccessibleName(/Not available yet/);
    expect(within(panel()).getByRole("button", { name: /Save to config\.toml/ })).toBeDisabled();
  });
});

describe("run settings", () => {
  it("shows what the plan says, read-only, with where each value comes from", async () => {
    setBackendForTests(null);
    const user = userEvent.setup();
    show();
    await open(user, "Parallel Execution");
    await waitFor(() => expect(row("Isolation Mode")).toHaveTextContent("worktree"));
    expect(row("Isolation Mode")).toHaveTextContent("plan.md");
    expect(row("Max Parallel Coders")).toHaveTextContent("2");
    expect(row("Risk Hold Gate")).toHaveTextContent("on");
    expect(row("Line Change Limit")).toHaveTextContent("1500");
    expect(row("Line Change Limit")).toHaveTextContent("ORCH_RISK_MAX_LINES");
    expect(row("Bounded Retry Caps")).toHaveTextContent("Fix: 3 rounds");
    for (const name of ["Isolation Mode", "Max Parallel Coders", "Scope Enforcement", "Risk Hold Gate"]) {
      expect(within(row(name)).queryByRole("textbox")).not.toBeInTheDocument();
      expect(within(row(name)).queryByRole("button")).not.toBeInTheDocument();
      expect(within(row(name)).queryByRole("combobox")).not.toBeInTheDocument();
    }
    await open(user, "Verification");
    expect(panel()).toHaveTextContent("cargo test");
    expect(panel()).toHaveTextContent("npm ci");
    await open(user, "Advanced TOML");
    expect(panel()).toHaveTextContent("Owner");
  });

  it("without a run: an empty state, and the app settings still work", async () => {
    server.respond("GET", "/plan", 404, errorBody("no_run", "no run"));
    setBackendForTests(null);
    const user = userEvent.setup();
    show();
    expect(await screen.findByText("No run in this project yet")).toBeInTheDocument();
    expect(within(row("Theme")).getByRole("button", { name: "Dark" })).toBeInTheDocument();
    await open(user, "Git & Worktrees");
    expect(row("Base Branch")).toHaveTextContent("No run yet");
  });

  it("a server that is down is an error with what to do", async () => {
    setBackendForTests(null);
    renderApp({ route: "/settings", initial: { url: await deadAddress(), token: "t" } });
    expect(await screen.findByText("Cannot reach the server")).toBeInTheDocument();
  });
});

describe("app settings", () => {
  it("in a browser only the theme is offered", async () => {
    setBackendForTests(null);
    const user = userEvent.setup();
    show();
    await screen.findByRole("tab", { name: "General" });
    await user.click(within(row("Theme")).getByRole("button", { name: "Light" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    await open(user, "Notifications");
    expect(row("Desktop Notifications")).toHaveTextContent("Available in the desktop app");
    expect(within(panel()).queryByLabelText(/Desktop notifications/)).not.toBeInTheDocument();
    await open(user, "Herdr Runtime");
    expect(within(panel()).queryByLabelText(/orch binary/)).not.toBeInTheDocument();
  });

  it("loads the saved settings and saves a change through the shell", async () => {
    const saveSettings = vi.fn<AppBackend["saveSettings"]>(async (s) => s);
    setBackendForTests(backend({ getSettings: async () => ({ value: settings({ theme: "dark", orch_path: "/opt/orch" }), warning: null }), saveSettings }));
    const user = userEvent.setup();
    show();
    await screen.findByRole("tab", { name: "General" });
    await waitFor(() => expect(within(row("Theme")).getByRole("button", { name: "Dark" })).toHaveAttribute("aria-pressed", "true"));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");

    await open(user, "Herdr Runtime");
    expect(within(panel()).getByLabelText("Path of the orch binary")).toHaveValue("/opt/orch");

    await open(user, "Notifications");
    await user.click(within(panel()).getByLabelText(/Desktop notifications/));
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(settings({ theme: "dark", orch_path: "/opt/orch", notifications: true })));
    expect(await within(panel()).findByText("The notification setting saved.")).toBeInTheDocument();

    await open(user, "General");
    await user.click(within(row("Theme")).getByRole("button", { name: "Light" }));
    await waitFor(() => expect(document.documentElement).toHaveAttribute("data-theme", "light"));
    expect(saveSettings).toHaveBeenLastCalledWith(settings({ theme: "light", orch_path: "/opt/orch", notifications: true }));
  });

  it("a failed save says so, never looks like success, and restores the field", async () => {
    const saveSettings = vi.fn<AppBackend["saveSettings"]>().mockRejectedValue(new Error("/nope is not an executable file: choose the orch binary"));
    // the stored theme is already the one on screen, so only the path is saved
    setBackendForTests(backend({ getSettings: async () => ({ value: settings({ theme: "dark" }), warning: null }), saveSettings }));
    const user = userEvent.setup();
    show();
    await open(user, "Herdr Runtime");
    const input = await within(panel()).findByLabelText("Path of the orch binary");
    await user.type(input, "/nope");
    await user.click(within(panel()).getByRole("button", { name: "Save path" }));
    expect(await within(panel()).findByRole("alert")).toHaveTextContent("The orch path was not saved: /nope is not an executable file");
    expect(within(panel()).queryByText(/saved\./)).not.toBeInTheDocument();
    expect(input).toHaveValue("");
  });

  it("a failed theme save does not change the theme", async () => {
    setBackendForTests(backend({ getSettings: async () => ({ value: settings({ theme: "dark" }), warning: null }), saveSettings: async () => Promise.reject(new Error("disk full")) }));
    const user = userEvent.setup();
    show();
    await screen.findByRole("tab", { name: "General" });
    expect(within(row("Theme")).getByRole("button", { name: "Dark" })).toHaveAttribute("aria-pressed", "true");
    await user.click(within(row("Theme")).getByRole("button", { name: "Light" }));
    expect(await within(panel()).findByRole("alert")).toHaveTextContent("The theme was not saved: disk full");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(within(row("Theme")).getByRole("button", { name: "Dark" })).toHaveAttribute("aria-pressed", "true");
  });

  it("opening Settings never changes the theme on screen, and the stored copy follows the top bar toggle", async () => {
    const saveSettings = vi.fn<AppBackend["saveSettings"]>(async (s) => s);
    // the shell still stores what was chosen long ago
    setBackendForTests(backend({ getSettings: async () => ({ value: settings({ theme: "system" }), warning: null }), saveSettings }));
    const user = userEvent.setup();
    show();
    await screen.findByRole("tab", { name: "General" });
    // the top bar toggle switches to light
    await user.click(screen.getByRole("button", { name: "Switch to light theme" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    // nothing jumps back, the control shows what is on screen, and the shell is brought in step
    await waitFor(() => expect(saveSettings).toHaveBeenLastCalledWith(settings({ theme: "light" })));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(within(row("Theme")).getByRole("button", { name: "Light" })).toHaveAttribute("aria-pressed", "true");
    expect(within(panel()).queryByRole("alert")).not.toBeInTheDocument();
  });

  it("on a fresh install the dark default stays, and the shell's default is updated to it", async () => {
    const saveSettings = vi.fn<AppBackend["saveSettings"]>(async (s) => s);
    setBackendForTests(backend({ getSettings: async () => ({ value: settings(), warning: null }), saveSettings }));
    show();
    await screen.findByRole("tab", { name: "General" });
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(settings({ theme: "dark" })));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(within(row("Theme")).getByRole("button", { name: "Dark" })).toHaveAttribute("aria-pressed", "true");
  });

  it("a theme the app cannot store in the shell is said so, and the theme on screen stays", async () => {
    setBackendForTests(backend({ getSettings: async () => ({ value: settings(), warning: null }), saveSettings: async () => Promise.reject(new Error("read-only disk")) }));
    show();
    await screen.findByRole("tab", { name: "General" });
    expect(await within(panel()).findByRole("alert")).toHaveTextContent("The theme could not be stored in the desktop settings: read-only disk");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("shows where orch was found, or says it was not", async () => {
    setBackendForTests(backend({ findOrch: async () => Promise.reject(new Error("not found")) }));
    const user = userEvent.setup();
    show();
    await open(user, "Herdr Runtime");
    expect(await screen.findByText(/orch was not found/)).toBeInTheDocument();
  });

  it("shows a warning when the saved settings were corrupt", async () => {
    setBackendForTests(backend({ getSettings: async () => ({ value: settings(), warning: "settings.txt could not be read; kept as settings.txt.corrupt" }) }));
    show();
    expect(await screen.findByText(/settings\.txt\.corrupt/)).toBeInTheDocument();
  });
});

describe("project settings", () => {
  it("AC1: the dropdown lists every agent CLI with Installed/Not installed mark, and missing agent cannot be chosen", async () => {
    const clis: AgentCli[] = [
      { kind: "claude", displayName: "Claude Code", installed: true, canLead: true },
      { kind: "codex", displayName: "Codex CLI", installed: false, canLead: true },
    ];
    setBackendForTests(
      backend({
        detectAgentClis: async () => clis,
        getProjectSettings: async () => ({ leadAgent: null, folder: "/home/me/project", warning: null }),
      }),
    );
    const user = userEvent.setup();
    show();
    await open(user, "Project");
    const select = await screen.findByRole("combobox", { name: "Default Lead Agent" });
    const options = within(select).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual([
      "None (no default lead agent)",
      "Claude Code (Installed)",
      "Codex CLI (Not installed)",
    ]);
    const missingOption = within(select).getByRole("option", { name: "Codex CLI (Not installed)" });
    expect(missingOption).toBeDisabled();

    // Selecting installed agent works
    await user.selectOptions(select, "claude");
    expect(select).toHaveValue("claude");
  });

  it("AC2: choosing an agent and saving shows it again after reloading the screen", async () => {
    let savedLead: string | null = null;
    const saveProjectSettings = vi.fn<NonNullable<AppBackend["saveProjectSettings"]>>().mockImplementation(async (_path, input) => {
      savedLead = input.leadAgent;
      return { leadAgent: input.leadAgent, folder: input.folder, warning: null };
    });
    const getProjectSettings = vi.fn<NonNullable<AppBackend["getProjectSettings"]>>().mockImplementation(async (path) => ({
      leadAgent: savedLead,
      folder: path,
      warning: null,
    }));
    const clis: AgentCli[] = [
      { kind: "claude", displayName: "Claude Code", installed: true, canLead: true },
    ];
    setBackendForTests(
      backend({
        detectAgentClis: async () => clis,
        getProjectSettings,
        saveProjectSettings,
      }),
    );
    const user = userEvent.setup();
    const { unmount } = show();
    await open(user, "Project");
    const select = await screen.findByRole("combobox", { name: "Default Lead Agent" });
    expect(select).toHaveValue("");
    await user.selectOptions(select, "claude");
    const saveBtn = screen.getByRole("button", { name: "Save project settings" });
    expect(saveBtn).toBeEnabled();
    await user.click(saveBtn);
    expect(saveProjectSettings).toHaveBeenCalledWith("/home/me/project", {
      leadAgent: "claude",
      folder: "/home/me/project",
    });
    expect(await screen.findByText("Project settings saved.")).toBeInTheDocument();

    // Reload the screen
    unmount();
    show();
    await open(user, "Project");
    const reloadedSelect = await screen.findByRole("combobox", { name: "Default Lead Agent" });
    expect(reloadedSelect).toHaveValue("claude");
  });

  it("AC2: a choice is kept while the screen re-renders, with a backend that is a new object on every call (as in the desktop app)", async () => {
    const getProjectSettings = vi.fn<NonNullable<AppBackend["getProjectSettings"]>>().mockImplementation(async (path) => ({
      leadAgent: null,
      folder: path,
      warning: null,
    }));
    const clis: AgentCli[] = [{ kind: "claude", displayName: "Claude Code", installed: true, canLead: true }];
    setBackendForTests(() => backend({ detectAgentClis: async () => clis, getProjectSettings }));
    const user = userEvent.setup();
    show();
    await open(user, "Project");
    const select = await screen.findByRole("combobox", { name: "Default Lead Agent" });
    await user.selectOptions(select, "claude");
    await waitFor(() => expect(screen.getByRole("button", { name: "Save project settings" })).toBeEnabled());
    expect(select).toHaveValue("claude");
    // loaded once, not again after every render
    expect(getProjectSettings).toHaveBeenCalledTimes(1);
  });

  it("AC3: a saved agent that is no longer installed is shown as missing with a hint to pick another, not silently dropped", async () => {
    const clis: AgentCli[] = [
      { kind: "claude", displayName: "Claude Code", installed: true, canLead: true },
      { kind: "codex", displayName: "Codex CLI", installed: false, canLead: true },
    ];
    setBackendForTests(
      backend({
        detectAgentClis: async () => clis,
        getProjectSettings: async (path) => ({ leadAgent: "codex", folder: path, warning: null }),
      }),
    );
    const user = userEvent.setup();
    show();
    await open(user, "Project");
    const select = await screen.findByRole("combobox", { name: "Default Lead Agent" });
    expect(select).toHaveValue("codex");
    const option = within(select).getByRole("option", { name: "Codex CLI (Not installed)" });
    expect(option).toBeDisabled();
    expect(option).toBeInTheDocument();
    expect(await screen.findByText(/The saved Lead agent “Codex CLI” is not installed\. Pick another Lead agent\./)).toBeInTheDocument();
  });

  it("AC4: save errors from the shell show next to the field in plain words and saving is disabled while nothing changed", async () => {
    const clis: AgentCli[] = [
      { kind: "claude", displayName: "Claude Code", installed: true, canLead: true },
    ];
    const saveProjectSettings = vi.fn<NonNullable<AppBackend["saveProjectSettings"]>>().mockRejectedValueOnce(
      new Error("/work/plain is not a git repository: choose the folder that contains .git (run `git init` there first if it is new)"),
    );
    setBackendForTests(
      backend({
        detectAgentClis: async () => clis,
        getProjectSettings: async (path) => ({ leadAgent: null, folder: path, warning: null }),
        saveProjectSettings,
      }),
    );
    const user = userEvent.setup();
    show();
    await open(user, "Project");
    const folderInput = await screen.findByRole("textbox", { name: "Project Folder" });
    const saveBtn = screen.getByRole("button", { name: "Save project settings" });
    // Saving is disabled while nothing changed
    expect(saveBtn).toBeDisabled();

    await user.clear(folderInput);
    await user.type(folderInput, "/work/plain");
    // Changed, saving is enabled
    expect(saveBtn).toBeEnabled();
    await user.click(saveBtn);

    // Save error from shell shows next to the field in plain words
    expect(await screen.findByText(/is not a git repository/)).toBeInTheDocument();
  });

  it("AC4: saving a folder change updates the project settings and remains loaded with success message", async () => {
    const saveProjectSettings = vi.fn<NonNullable<AppBackend["saveProjectSettings"]>>().mockImplementation(async (_path, input) => ({
      leadAgent: input.leadAgent,
      folder: input.folder,
      warning: null,
    }));
    const switchProject = vi.fn<AppBackend["switchProject"]>().mockImplementation(async (path) => {
      server.respond("GET", "/health", 200, { ...samples.health, project: path });
      return { url: server.url, token: server.token };
    });
    const getProjectSettings = vi.fn<NonNullable<AppBackend["getProjectSettings"]>>().mockImplementation(async (path) => ({
      leadAgent: null,
      folder: path,
      warning: null,
    }));
    setBackendForTests(
      backend({
        detectAgentClis: async () => [{ kind: "claude", displayName: "Claude Code", installed: true, canLead: true }],
        getProjectSettings,
        saveProjectSettings,
        switchProject,
      }),
    );
    const user = userEvent.setup();
    show();
    await open(user, "Project");
    const folderInput = await screen.findByRole("textbox", { name: "Project Folder" });
    const saveBtn = screen.getByRole("button", { name: "Save project settings" });
    expect(saveBtn).toBeDisabled();

    await user.clear(folderInput);
    await user.type(folderInput, "/work/moved");
    expect(saveBtn).toBeEnabled();
    await user.click(saveBtn);

    expect(saveProjectSettings).toHaveBeenCalledWith("/home/me/project", {
      leadAgent: null,
      folder: "/work/moved",
    });
    await waitFor(() => expect(switchProject).toHaveBeenCalledWith("/work/moved"));
    expect(await screen.findByText("Project settings saved.")).toBeInTheDocument();
    expect(folderInput).toHaveValue("/work/moved");
    expect(saveBtn).toBeDisabled();
    expect(screen.queryByText("Loading project settings…")).not.toBeInTheDocument();
  });

  it("AC4: a failed load shows the error message with a retry button, does not get stuck loading, and retrying loads the settings", async () => {
    let fail = true;
    const getProjectSettings = vi.fn<NonNullable<AppBackend["getProjectSettings"]>>().mockImplementation(async (path) => {
      if (fail) throw new Error("/home/me/project is not in the project list");
      return { leadAgent: "claude", folder: path, warning: null };
    });
    setBackendForTests(
      backend({
        detectAgentClis: async () => [{ kind: "claude", displayName: "Claude Code", installed: true, canLead: true }],
        getProjectSettings,
      }),
    );
    const user = userEvent.setup();
    show();
    await open(user, "Project");

    expect(await screen.findByRole("alert")).toHaveTextContent("/home/me/project is not in the project list");
    expect(screen.queryByText("Loading project settings…")).not.toBeInTheDocument();
    const retryBtn = screen.getByRole("button", { name: "Try again" });
    expect(retryBtn).toBeInTheDocument();

    fail = false;
    await user.click(retryBtn);
    const select = await screen.findByRole("combobox", { name: "Default Lead Agent" });
    expect(select).toHaveValue("claude");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("AC5: the Project section works with keyboard only (select, folder input, save)", async () => {
    const saveProjectSettings = vi.fn<NonNullable<AppBackend["saveProjectSettings"]>>().mockImplementation(async (_path, input) => ({
      leadAgent: input.leadAgent,
      folder: input.folder,
      warning: null,
    }));
    setBackendForTests(
      backend({
        detectAgentClis: async () => [
          { kind: "claude", displayName: "Claude Code", installed: true, canLead: true },
          { kind: "codex", displayName: "Codex CLI", installed: false, canLead: true },
        ],
        getProjectSettings: async (path) => ({ leadAgent: null, folder: path, warning: null }),
        saveProjectSettings,
      }),
    );
    const user = userEvent.setup();
    show();

    // Navigate to Project tab using keyboard
    const tabList = await screen.findByRole("tablist", { name: "Settings sections" });
    within(tabList).getByRole("tab", { name: "General" }).focus();
    await user.keyboard("{ArrowDown}");
    expect(within(tabList).getByRole("tab", { name: "Project" })).toHaveFocus();

    // Tab into the Lead Agent select
    const select = await screen.findByRole("combobox", { name: "Default Lead Agent" });
    select.focus();
    expect(select).toHaveFocus();
    await user.selectOptions(select, "claude");
    expect(select).toHaveValue("claude");

    // Tab into the Folder input
    const folderInput = screen.getByRole("textbox", { name: "Project Folder" });
    await user.tab();
    expect(folderInput).toHaveFocus();
    await user.clear(folderInput);
    await user.type(folderInput, "/home/me/updated");
    expect(folderInput).toHaveValue("/home/me/updated");

    // Tab to Save button and press Enter
    const saveBtn = screen.getByRole("button", { name: "Save project settings" });
    await user.tab();
    expect(saveBtn).toHaveFocus();
    expect(saveBtn).toBeEnabled();
    await user.keyboard("{Enter}");

    await waitFor(() =>
      expect(saveProjectSettings).toHaveBeenCalledWith("/home/me/project", {
        leadAgent: "claude",
        folder: "/home/me/updated",
      }),
    );
    expect(await screen.findByText("Project settings saved.")).toBeInTheDocument();
  });

  it("AC5: in a browser the project section layout is visible with controls marked desktop-only", async () => {
    setBackendForTests(null);
    const user = userEvent.setup();
    show();
    await open(user, "Project");
    const select = await screen.findByRole("combobox", { name: "Default Lead Agent" });
    expect(select).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Project Folder" })).toBeDisabled();
    const saveBtn = screen.getByRole("button", { name: /Save project settings/ });
    expect(saveBtn).toBeDisabled();
    expect(saveBtn).toHaveAttribute("title", "Available in the desktop app");
  });

  it("AC6: when no project is open the section says so instead of showing empty fields", async () => {
    server.respond("GET", "/health", 200, { ...samples.health, project: "" });
    setBackendForTests(
      backend({
        listProjects: async () => ({ projects: [], active: null, open: [], warning: null }),
        restoreProjects: async () => [],
      }),
    );
    const user = userEvent.setup();
    show();
    await open(user, "Project");
    expect(await screen.findByText("No project is open")).toBeInTheDocument();
    expect(screen.getByText(/Open a project to configure its settings/)).toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Default Lead Agent" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Project Folder" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save project settings" })).not.toBeInTheDocument();
  });
});
