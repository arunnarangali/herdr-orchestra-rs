import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { samples } from "../../api/contract";
import { deadAddress, errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { setBackendForTests, type AppBackend, type AppSettings } from "../projects/backend";

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
      "General", "Herdr Runtime", "Agent Providers", "Agent Roles", "Models & Policy", "Lead Mode", "Scheduling",
      "Parallel Execution", "Git & Worktrees", "Verification", "Review", "Risk Hold & Scope", "Recovery & Retries",
      "Notifications", "Storage", "Advanced TOML",
    ]);
    tab("General").focus();
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
