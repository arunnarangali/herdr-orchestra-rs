import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { samples } from "../../api/contract";
import { deadAddress, errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { setBackendForTests, type AppBackend, type ProjectInfo, type ProjectList } from "./backend";

const project = (o: Partial<ProjectInfo> = {}): ProjectInfo => ({
  path: "/home/me/project",
  name: "project",
  exists: true,
  is_git: true,
  branch: "main",
  has_orchestrate: true,
  archive: [],
  ...o,
});
const listOf = (projects: ProjectInfo[], active: string | null = null, warning: string | null = null): ProjectList => ({ projects, active, warning });

function backend(o: Partial<AppBackend> = {}): AppBackend {
  return {
    chooseFolder: async () => null,
    listProjects: async () => listOf([]),
    addProject: async () => listOf([]),
    removeProject: async () => listOf([]),
    switchProject: async () => {
      throw new Error("switch not set up");
    },
    getSettings: async () => ({ value: { theme: "system", notifications: false, orch_path: "" }, warning: null }),
    saveSettings: async (s) => s,
    findOrch: async () => "/usr/bin/orch",
    ...o,
  };
}

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  server.respond("GET", "/status", 200, { ...samples.status, base: "main", run_state: "running" });
  server.respond("GET", "/report", 200, samples.report);
});
afterEach(async () => {
  setBackendForTests(undefined);
  await server.close();
});

const show = () => renderApp({ route: "/projects", initial: server });

describe("Projects", () => {
  it("in a browser: says it is connected to one server and offers no list", async () => {
    setBackendForTests(null);
    show();
    expect(await screen.findByText("Connected to one server")).toBeInTheDocument();
    expect(await screen.findByText("/home/me/project")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add project…" })).not.toBeInTheDocument();
  });

  it("no project: an empty state that says how to add one", async () => {
    setBackendForTests(backend());
    show();
    expect(await screen.findByText("No projects yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add project…" })).toBeInTheDocument();
  });

  it("shows path, branch, base, run state, report and earlier runs for the active project", async () => {
    const p = project({
      archive: [{ name: "phase-1", files: ["plan.md", "summary.md"], modified: 1_700_000_000, summary: "# Phase 1 <b>done</b>" }],
    });
    const other = project({ path: "/home/me/other", name: "other", branch: "dev" });
    setBackendForTests(backend({ listProjects: async () => listOf([p, other], p.path) }));
    show();
    const card = await screen.findByRole("article", { name: "project" });
    expect(card).toHaveTextContent("/home/me/project");
    expect(card).toHaveTextContent("main");
    expect(within(card).getByText("Active")).toBeInTheDocument();
    await waitFor(() => expect(card).toHaveTextContent("orch/desktop from main"));
    expect(card).toHaveTextContent("Running");
    expect(card).toHaveTextContent("1 of 2 merged");
    expect(card).toHaveTextContent("phase-1");
    expect(within(card).getByText("# Phase 1 <b>done</b>")).toBeInTheDocument(); // plain text
    const otherCard = screen.getByRole("article", { name: "other" });
    expect(otherCard).toHaveTextContent("dev");
    expect(within(otherCard).getByText("No earlier runs in .orchestrate/archive/.")).toBeInTheDocument();
    expect(within(otherCard).getByRole("button", { name: "Switch to this project" })).toBeEnabled();
    expect(within(card).queryByRole("button", { name: "Switch to this project" })).not.toBeInTheDocument();
  });

  it("a project without a run says so", async () => {
    server.respond("GET", "/status", 404, errorBody("no_run", "no run"));
    server.respond("GET", "/report", 404, errorBody("no_run", "no run"));
    const p = project({ has_orchestrate: false });
    setBackendForTests(backend({ listProjects: async () => listOf([p], p.path) }));
    show();
    expect(await screen.findByText(/No run yet/)).toBeInTheDocument();
  });

  it("a server that is down is said so on the active project", async () => {
    const dead = await deadAddress();
    const p = project();
    setBackendForTests(backend({ listProjects: async () => listOf([p], p.path) }));
    renderApp({ route: "/projects", initial: { url: dead, token: "t" } });
    expect(await screen.findByText("The server is not answering.")).toBeInTheDocument();
  });

  it("a stored list that was corrupt shows the warning", async () => {
    setBackendForTests(backend({ listProjects: async () => listOf([], null, "projects.txt could not be read; it was kept as projects.txt.corrupt.") }));
    show();
    expect(await screen.findByRole("alert")).toHaveTextContent("projects.txt.corrupt");
  });

  it("adds a folder; a folder that is not a git repository is refused with the message", async () => {
    const user = userEvent.setup();
    const addProject = vi
      .fn<AppBackend["addProject"]>()
      .mockRejectedValueOnce(new Error("/work/plain is not a git repository: choose the folder that contains .git"))
      .mockResolvedValueOnce(listOf([project({ path: "/work/repo", name: "repo" })]));
    const chooseFolder = vi.fn<AppBackend["chooseFolder"]>().mockResolvedValueOnce("/work/plain").mockResolvedValueOnce("/work/repo");
    setBackendForTests(backend({ addProject, chooseFolder }));
    show();
    await user.click(await screen.findByRole("button", { name: "Add project…" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("is not a git repository");
    expect(screen.getByText("No projects yet")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add project…" }));
    expect(await screen.findByRole("article", { name: "repo" })).toBeInTheDocument();
    expect(screen.getByText("Added /work/repo.")).toBeInTheDocument();
  });

  it("cancelling the folder dialog changes nothing", async () => {
    const user = userEvent.setup();
    const addProject = vi.fn<AppBackend["addProject"]>();
    setBackendForTests(backend({ addProject }));
    show();
    await user.click(await screen.findByRole("button", { name: "Add project…" }));
    expect(addProject).not.toHaveBeenCalled();
  });

  it("removing asks first and says the files are not deleted", async () => {
    const user = userEvent.setup();
    const p = project();
    const removeProject = vi.fn<AppBackend["removeProject"]>().mockResolvedValue(listOf([]));
    setBackendForTests(backend({ listProjects: async () => listOf([p]), removeProject }));
    show();
    await user.click(await screen.findByRole("button", { name: "Remove project from the list" }));
    const dialog = screen.getByRole("alertdialog");
    expect(dialog).toHaveTextContent("files are not deleted");
    expect(removeProject).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(removeProject).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Remove project from the list" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Remove from list" }));
    await waitFor(() => expect(removeProject).toHaveBeenCalledWith("/home/me/project"));
    expect(await screen.findByText("No projects yet")).toBeInTheDocument();
  });

  it("switching restarts the server for the project and connects to what the shell returns", async () => {
    const user = userEvent.setup();
    const a = project();
    const b = project({ path: "/home/me/other", name: "other" });
    const switchProject = vi.fn<AppBackend["switchProject"]>().mockResolvedValue({ url: server.url, token: server.token });
    setBackendForTests(
      backend({
        listProjects: async () => listOf([a, b], a.path),
        switchProject,
      }),
    );
    show();
    const card = await screen.findByRole("article", { name: "other" });
    await user.click(within(card).getByRole("button", { name: "Switch to this project" }));
    await waitFor(() => expect(switchProject).toHaveBeenCalledWith("/home/me/other"));
    expect(await screen.findByText(/Switched to other/)).toBeInTheDocument();
  });

  it("a folder that vanished is marked and cannot be switched to, but can be removed", async () => {
    const gone = project({ path: "/gone", name: "gone", exists: false, is_git: false, branch: null, archive: [] });
    setBackendForTests(backend({ listProjects: async () => listOf([project(), gone], "/home/me/project") }));
    show();
    const card = await screen.findByRole("article", { name: "gone" });
    expect(within(card).getByText("Folder not found")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Switch to this project" })).toBeDisabled();
    expect(within(card).getByRole("button", { name: "Remove gone from the list" })).toBeEnabled();
  });

  it("a failed switch shows the reason and does not claim success", async () => {
    const user = userEvent.setup();
    const b = project({ path: "/home/me/other", name: "other" });
    setBackendForTests(
      backend({
        listProjects: async () => listOf([project(), b], "/home/me/project"),
        switchProject: async () => {
          throw new Error("orch was not found: set ORCH_BIN or put it on your PATH");
        },
      }),
    );
    show();
    const card = await screen.findByRole("article", { name: "other" });
    await user.click(within(card).getByRole("button", { name: "Switch to this project" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("set ORCH_BIN");
    expect(screen.queryByText(/Switched to/)).not.toBeInTheDocument();
  });
});
