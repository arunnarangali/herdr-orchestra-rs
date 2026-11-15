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
    listProjects: async () => ({ projects: [], active: null, warning: null }),
    addProject: async () => ({ projects: [], active: null, warning: null }),
    removeProject: async () => ({ projects: [], active: null, warning: null }),
    switchProject: async () => {
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

describe("run settings", () => {
  it("shows what the API gives, read-only, with a note that they come from plan.md", async () => {
    setBackendForTests(null);
    show();
    const run = await screen.findByRole("region", { name: "Run settings" });
    expect(run).toHaveTextContent("worktree");
    expect(run).toHaveTextContent("Max parallel");
    expect(run).toHaveTextContent("Risk hold");
    expect(run).toHaveTextContent("cargo test");
    expect(run).toHaveTextContent("npm ci");
    expect(run).toHaveTextContent("Owner");
    expect(within(run).getByRole("note")).toHaveTextContent("come from the header of plan.md");
    expect(within(run).queryByRole("textbox")).not.toBeInTheDocument();
    expect(within(run).queryByRole("button")).not.toBeInTheDocument();
    expect(within(run).queryByRole("combobox")).not.toBeInTheDocument();
  });

  it("without a run: an empty state, and the app settings still work", async () => {
    server.respond("GET", "/plan", 404, errorBody("no_run", "no run"));
    setBackendForTests(null);
    show();
    expect(await screen.findByText("No run in this project yet")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "App settings" })).toBeInTheDocument();
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
    show();
    const app = await screen.findByRole("region", { name: "App settings" });
    expect(within(app).getByLabelText("Theme")).toBeInTheDocument();
    expect(within(app).queryByLabelText(/Desktop notifications/)).not.toBeInTheDocument();
    expect(within(app).queryByLabelText(/orch binary/)).not.toBeInTheDocument();
    expect(within(app).getByRole("note")).toHaveTextContent("desktop app only");
  });

  it("loads the saved settings and saves a change through the shell", async () => {
    const saveSettings = vi.fn<AppBackend["saveSettings"]>(async (s) => s);
    setBackendForTests(backend({ getSettings: async () => ({ value: settings({ theme: "dark", orch_path: "/opt/orch" }), warning: null }), saveSettings }));
    const user = userEvent.setup();
    show();
    const app = await screen.findByRole("region", { name: "App settings" });
    await waitFor(() => expect(within(app).getByLabelText("Path of the orch binary")).toHaveValue("/opt/orch"));
    expect(within(app).getByLabelText("Theme")).toHaveValue("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");

    await user.click(within(app).getByLabelText(/Desktop notifications/));
    await waitFor(() => expect(saveSettings).toHaveBeenCalledWith(settings({ theme: "dark", orch_path: "/opt/orch", notifications: true })));
    expect(await within(app).findByText("The notification setting saved.")).toBeInTheDocument();

    await user.selectOptions(within(app).getByLabelText("Theme"), "light");
    await waitFor(() => expect(document.documentElement).toHaveAttribute("data-theme", "light"));
    expect(saveSettings).toHaveBeenLastCalledWith(settings({ theme: "light", orch_path: "/opt/orch", notifications: true }));
  });

  it("a failed save says so, never looks like success, and restores the field", async () => {
    const saveSettings = vi.fn<AppBackend["saveSettings"]>().mockRejectedValue(new Error("/nope is not an executable file: choose the orch binary"));
    setBackendForTests(backend({ saveSettings }));
    const user = userEvent.setup();
    show();
    const app = await screen.findByRole("region", { name: "App settings" });
    const input = await within(app).findByLabelText("Path of the orch binary");
    await user.type(input, "/nope");
    await user.click(within(app).getByRole("button", { name: "Save path" }));
    expect(await within(app).findByRole("alert")).toHaveTextContent("The orch path was not saved: /nope is not an executable file");
    expect(within(app).queryByText(/saved\./)).not.toBeInTheDocument();
    expect(input).toHaveValue("");
  });

  it("a failed theme save does not change the theme", async () => {
    setBackendForTests(backend({ saveSettings: async () => Promise.reject(new Error("disk full")) }));
    const user = userEvent.setup();
    show();
    const app = await screen.findByRole("region", { name: "App settings" });
    await user.selectOptions(await within(app).findByLabelText("Theme"), "dark");
    expect(await within(app).findByRole("alert")).toHaveTextContent("disk full");
    expect(document.documentElement).not.toHaveAttribute("data-theme", "dark");
  });

  it("shows where orch was found, or says it was not", async () => {
    setBackendForTests(backend({ findOrch: async () => Promise.reject(new Error("not found")) }));
    show();
    expect(await screen.findByText(/orch was not found/)).toBeInTheDocument();
  });

  it("shows a warning when the saved settings were corrupt", async () => {
    setBackendForTests(backend({ getSettings: async () => ({ value: settings(), warning: "settings.txt could not be read; kept as settings.txt.corrupt" }) }));
    show();
    expect(await screen.findByText(/settings\.txt\.corrupt/)).toBeInTheDocument();
  });
});
