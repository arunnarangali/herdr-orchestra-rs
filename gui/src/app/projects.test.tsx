import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../api/contract";
import type { AttentionItem, ChatMessage } from "../api/types";
import {
  setBackendForTests,
  type AppBackend,
  type ProjectInfo,
  type ProjectList,
  type RestoredProject,
} from "../screens/projects/backend";
import { startFakeServer, type FakeServer } from "../test/fakeServer";
import { renderApp } from "../test/render";

const PA = "/work/alpha";
const PB = "/work/beta";

const chat = (text: string, id = 1): ChatMessage => ({
  id,
  ts: "2026-11-15T09:30:00Z",
  sender: "lead",
  kind: "message",
  text,
  request: null,
  about: null,
  delivery: null,
  delivery_note: null,
});
const info = (path: string): ProjectInfo => ({
  path,
  name: path.split("/").pop()!,
  exists: true,
  is_git: true,
  branch: "main",
  has_orchestrate: true,
  archive: [],
});
const listOf = (paths: string[], open: string[] = paths, active: string | null = null): ProjectList => ({
  projects: paths.map(info),
  active,
  open,
  warning: null,
});
const attention = (o: Partial<AttentionItem> = {}): AttentionItem => ({
  id: "item-1",
  kind: "question",
  task: "M1",
  agent: "coder-1",
  title: "Question on M1",
  detail: "YAML or JSON?",
  prompt: null,
  since: "2026-11-15T12:00:00Z",
  actions: ["answer"],
  ...o,
});

let A: FakeServer;
let B: FakeServer;
const creds = (s: FakeServer) => ({ url: s.url, token: s.token });
const restored = (...r: Array<[string, FakeServer | null, string?]>): RestoredProject[] =>
  r.map(([path, s, error]) => ({ path, name: path.split("/").pop()!, connection: s ? creds(s) : null, error: error ?? null }));

function backend(o: Partial<AppBackend> = {}): AppBackend & { calls: string[] } {
  const calls: string[] = [];
  const b: AppBackend & { calls: string[] } = {
    calls,
    chooseFolder: async () => null,
    listProjects: async () => listOf([PA, PB], [PA, PB], PA),
    addProject: async () => listOf([PA, PB]),
    removeProject: async () => listOf([PA, PB]),
    switchProject: async (p) => {
      calls.push(`switch ${p}`);
      return creds(p === PA ? A : B);
    },
    openProject: async (p) => {
      calls.push(`open ${p}`);
      return creds(p === PA ? A : B);
    },
    retryProject: async (p) => {
      calls.push(`retry ${p}`);
      return creds(p === PA ? A : B);
    },
    closeProject: async (p) => {
      calls.push(`close ${p}`);
      return listOf([PA, PB], [PA, PB].filter((x) => x !== p));
    },
    restoreProjects: async () => restored([PA, A], [PB, B]),
    createProject: async () => {
      throw new Error("create not set up");
    },
    getSettings: async () => ({ value: { theme: "system", notifications: false, orch_path: "" }, warning: null }),
    saveSettings: async (s) => s,
    findOrch: async () => "/usr/bin/orch",
    ...o,
  };
  return b;
}

beforeEach(async () => {
  A = await startFakeServer({ token: "a".repeat(64) });
  B = await startFakeServer({ token: "b".repeat(64) });
  for (const [s, text] of [
    [A, "alpha says hello"],
    [B, "beta says hello"],
  ] as const) {
    s.respond("GET", "/chat", 200, { messages: [chat(text)] });
    s.respond("GET", "/attention", 200, { items: [] });
    s.respond("GET", "/status", 200, { ...samples.status, run_state: "running" });
  }
  B.respond("GET", "/status", 200, { ...samples.status, run_state: "all_done" });
});
afterEach(async () => {
  setBackendForTests(undefined);
  await Promise.all([A.close(), B.close()]);
});

const open = (route = "/chat", b = backend()) => {
  setBackendForTests(b);
  renderApp({ route });
  return b;
};
const tabs = () => screen.getByRole("navigation", { name: "Open projects" });
const tab = (name: string) => within(tabs()).getByRole("button", { name: new RegExp(`^${name}`) });
const gets = (s: FakeServer, path: string) => s.requests.filter((r) => r.method === "GET" && r.path === path).length;
const settled = () => new Promise((r) => setTimeout(r, 250));

describe("open projects", () => {
  it("reopens the projects that were open, shows each with its run state, and puts the active one on screen", async () => {
    open();
    expect(await screen.findByText("alpha says hello")).toBeInTheDocument();
    await waitFor(() => expect(tabs()).toHaveTextContent("All done")); // beta's run state, read in the background
    expect(tab("alpha")).toHaveAttribute("aria-current", "true");
    expect(tab("beta")).not.toHaveAttribute("aria-current");
    expect(tab("alpha")).toHaveTextContent("Running");
    expect(screen.getAllByText("alpha").length).toBeGreaterThan(0); // the project on screen is named
  });

  it("shows 'Opening your projects…' while restoring, not the connect form", async () => {
    let release: (r: RestoredProject[]) => void = () => {};
    open("/chat", backend({ restoreProjects: () => new Promise((r) => (release = r)) }));
    expect(await screen.findByText("Opening your projects…")).toBeInTheDocument();
    expect(screen.queryByText(/Getting started/)).not.toBeInTheDocument();
    await act(async () => release(restored([PA, A])));
    expect(await screen.findByText("alpha says hello")).toBeInTheDocument();
  });

  it("with nothing open it falls back to Getting started", async () => {
    open("/chat", backend({ restoreProjects: async () => [] }));
    expect(await screen.findByRole("heading", { name: "Getting started" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Open projects" })).not.toBeInTheDocument();
  });
});

describe("switching: nothing of project A shows in project B", () => {
  it("switching changes the whole screen to that project, and A's server is not asked for it", async () => {
    const user = userEvent.setup();
    const b = open();
    await screen.findByText("alpha says hello");
    const chatReadsOfA = gets(A, "/chat");
    await user.click(tab("beta"));
    expect(await screen.findByText("beta says hello")).toBeInTheDocument();
    expect(screen.queryByText("alpha says hello")).not.toBeInTheDocument();
    expect(tab("beta")).toHaveAttribute("aria-current", "true");
    expect(b.calls).toContain(`switch ${PB}`); // remembered for the next start
    await settled();
    expect(gets(A, "/chat")).toBe(chatReadsOfA); // the shown screen asked B, never A
    expect(gets(B, "/chat")).toBeGreaterThan(0);
  });

  it("every request of the shown screen carries the shown project's token", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText("alpha says hello");
    await user.click(tab("beta"));
    await screen.findByText("beta says hello");
    await settled();
    const chatReads = (s: FakeServer) => s.requests.filter((r) => r.path === "/chat");
    expect(chatReads(A).every((r) => r.headers.authorization === `Bearer ${A.token}`)).toBe(true);
    expect(chatReads(B).every((r) => r.headers.authorization === `Bearer ${B.token}`)).toBe(true);
  });

  it("a live event of project A never shows while B is on screen", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText("alpha says hello");
    await user.click(tab("beta"));
    await screen.findByText("beta says hello");
    await waitFor(() => expect(B.streamRequests().length).toBeGreaterThan(0));
    await act(async () => A.emit(1, "chat", chat("alpha secret", 2)));
    await settled();
    expect(screen.queryByText("alpha secret")).not.toBeInTheDocument();
    // and B's own event does show
    await act(async () => B.emit(1, "chat", chat("beta news", 2)));
    expect(await screen.findByText("beta news")).toBeInTheDocument();
    // switching back: A's screen is built from A's server again, and has A's event, not B's
    A.respond("GET", "/chat", 200, { messages: [chat("alpha says hello"), chat("alpha secret", 2)] });
    await user.click(tab("alpha"));
    expect(await screen.findByText("alpha secret")).toBeInTheDocument();
    expect(screen.queryByText("beta news")).not.toBeInTheDocument();
    expect(screen.queryByText("beta says hello")).not.toBeInTheDocument();
  });

  it("after a quick switch back and forth only the last project's data is on screen", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText("alpha says hello");
    for (const name of ["beta", "alpha", "beta", "alpha", "beta"]) await user.click(tab(name));
    expect(await screen.findByText("beta says hello")).toBeInTheDocument();
    await settled();
    expect(screen.queryByText("alpha says hello")).not.toBeInTheDocument();
    expect(tab("beta")).toHaveAttribute("aria-current", "true");
  });

  it("a slow answer from A that arrives after switching to B is dropped", async () => {
    const user = userEvent.setup();
    // A's chat read hangs until released, then answers
    const slow = await startFakeServer({ token: "c".repeat(64) });
    slow.respond("GET", "/attention", 200, { items: [] });
    slow.respond("GET", "/status", 200, samples.status);
    slow.respond("GET", "/chat", 200, { messages: [chat("late alpha answer")] });
    const realFetch = globalThis.fetch;
    const gate: { open: () => void; asked: boolean } = { open: () => {}, asked: false };
    const held = new Promise<void>((r) => (gate.open = r));
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.startsWith(slow.url) && url.endsWith("/chat")) {
        gate.asked = true;
        await held;
      }
      return realFetch(input, init);
    }) as typeof fetch;
    try {
      open("/chat", backend({ restoreProjects: async () => restored([PA, slow], [PB, B]) }));
      await waitFor(() => expect(gate.asked).toBe(true));
      await user.click(tab("beta"));
      expect(await screen.findByText("beta says hello")).toBeInTheDocument();
      gate.open();
      await settled();
      expect(screen.queryByText("late alpha answer")).not.toBeInTheDocument();
    } finally {
      globalThis.fetch = realFetch;
      await slow.close();
    }
  });

  it("after the streams reconnect, B still shows only B", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText("alpha says hello");
    await user.click(tab("beta"));
    await screen.findByText("beta says hello");
    await waitFor(() => expect(A.streamRequests().length).toBeGreaterThan(0));
    const before = { a: A.streamRequests().length, b: B.streamRequests().length };
    A.dropStreams();
    B.dropStreams();
    await waitFor(() => {
      expect(A.streamRequests().length).toBeGreaterThan(before.a);
      expect(B.streamRequests().length).toBeGreaterThan(before.b);
    });
    await settled();
    await act(async () => A.emit(50, "chat", chat("alpha after reconnect", 9)));
    await settled();
    expect(screen.queryByText("alpha after reconnect")).not.toBeInTheDocument();
    expect(screen.getByText("beta says hello")).toBeInTheDocument();
    await act(async () => B.emit(50, "chat", chat("beta after reconnect", 9)));
    expect(await screen.findByText("beta after reconnect")).toBeInTheDocument();
  });
});

describe("attention from every open project", () => {
  it("raises the popup for a project that is not on screen, names it, and acts on that project only", async () => {
    const user = userEvent.setup();
    A.respond("POST", "/tasks/M1/answer", 200, { ...samples.action, message: "answered" });
    open(); // alpha is on screen, the item comes from beta
    await screen.findByText("alpha says hello");
    await waitFor(() => expect(B.streamRequests().length).toBeGreaterThan(0));
    await act(async () => B.emit(1, "attention", { op: "add", item: attention({ id: "b-1", title: "Beta needs you" }) }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Beta needs you");
    expect(dialog).toHaveTextContent("Project: beta");
    // the indicator and the beta tab count it; alpha's tab does not
    expect(screen.getByRole("button", { name: /Notifications, 1 need attention/, hidden: true })).toBeInTheDocument();
    const betaTab = within(tabs()).getByRole("button", { name: /^beta/, hidden: true });
    const alphaTab = within(tabs()).getByRole("button", { name: /^alpha/, hidden: true });
    expect(within(betaTab).getByLabelText(/1 item needs attention/)).toBeInTheDocument();
    expect(within(alphaTab).queryByLabelText(/need/)).not.toBeInTheDocument();

    B.respond("POST", "/tasks/M1/answer", 200, { ...samples.action, message: "answered" });
    await user.type(within(dialog).getByLabelText("Your answer:"), "JSON");
    await user.click(within(dialog).getByRole("button", { name: "Send answer" }));
    await waitFor(() => expect(B.requests.some((r) => r.method === "POST" && r.path === "/tasks/M1/answer")).toBe(true));
    expect(A.requests.some((r) => r.method === "POST")).toBe(false); // acting never touches the other project
  });

  it("two projects may both have an item with the same id: they stay two items", async () => {
    open();
    await screen.findByText("alpha says hello");
    await waitFor(() => expect(A.streamRequests().length + B.streamRequests().length).toBeGreaterThan(1));
    await act(async () => {
      A.emit(1, "attention", { op: "add", item: attention({ title: "Alpha question" }) });
      B.emit(1, "attention", { op: "add", item: attention({ title: "Beta question" }) });
    });
    expect(await screen.findByRole("button", { name: /Notifications, 2 need attention/, hidden: true })).toBeInTheDocument();
    // removing alpha's leaves beta's
    await act(async () => A.emit(2, "attention", { op: "remove", item: attention() }));
    expect(await screen.findByRole("button", { name: /Notifications, 1 need attention/, hidden: true })).toBeInTheDocument();
  });

  it("items that were already waiting when the app started are counted per project", async () => {
    B.respond("GET", "/attention", 200, { items: [attention({ id: "x", title: "Waiting in beta" })] });
    open();
    await screen.findByText("alpha says hello");
    expect(await within(tabs()).findByLabelText(/1 item needs attention/)).toBeInTheDocument();
    expect(within(tab("beta")).getByLabelText(/1 item/)).toBeInTheDocument();
    expect(within(tab("alpha")).queryByLabelText(/need/)).not.toBeInTheDocument();
  });

  it("closing a project takes its items away", async () => {
    const user = userEvent.setup();
    B.respond("GET", "/attention", 200, { items: [attention({ id: "x", title: "Waiting in beta" })] });
    open();
    await within(await screen.findByRole("navigation", { name: "Open projects" })).findByLabelText(/1 item needs attention/);
    await user.click(within(tabs()).getByRole("button", { name: "Close beta" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: /need attention/ })).not.toBeInTheDocument());
  });
});

describe("a project that does not start", () => {
  it("shows why and a retry button, and the others are not affected", async () => {
    const user = userEvent.setup();
    const b = open(
      "/chat",
      backend({ restoreProjects: async () => restored([PA, A], [PB, null, "orch serve did not start: address in use"]) }),
    );
    expect(await screen.findByText("alpha says hello")).toBeInTheDocument();
    expect(tab("beta")).toHaveTextContent("Did not start");
    await user.click(tab("beta"));
    const problem = await screen.findByRole("alert");
    expect(problem).toHaveTextContent("beta did not start");
    expect(problem).toHaveTextContent("address in use");
    expect(problem).toHaveTextContent("the others keep running");
    // alpha's tab is still healthy and works
    expect(tab("alpha")).toHaveTextContent("Running");

    await user.click(within(problem).getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("beta says hello")).toBeInTheDocument();
    expect(b.calls).toContain(`retry ${PB}`);
    expect(tab("beta")).not.toHaveTextContent("Did not start");
  });

  it("a retry that fails again keeps the reason", async () => {
    const user = userEvent.setup();
    open(
      "/chat",
      backend({
        restoreProjects: async () => restored([PA, A], [PB, null, "first reason"]),
        retryProject: async () => {
          throw new Error("second reason: orch not found");
        },
      }),
    );
    await screen.findByText("alpha says hello");
    await user.click(tab("beta"));
    await user.click(await screen.findByRole("button", { name: "Try again" }));
    expect(await screen.findByText(/second reason: orch not found/)).toBeInTheDocument();
  });
});

describe("closing", () => {
  it("closing the project on screen stops its server (through the shell) and shows another one", async () => {
    const user = userEvent.setup();
    const b = open();
    await screen.findByText("alpha says hello");
    await user.click(within(tabs()).getByRole("button", { name: "Close alpha" }));
    expect(await screen.findByText("beta says hello")).toBeInTheDocument();
    expect(b.calls).toContain(`close ${PA}`);
    expect(within(tabs()).queryByRole("button", { name: /^alpha/ })).not.toBeInTheDocument();
  });

  it("closing the last project goes back to Getting started", async () => {
    const user = userEvent.setup();
    open("/chat", backend({ restoreProjects: async () => restored([PA, A]) }));
    await screen.findByText("alpha says hello");
    await user.click(within(tabs()).getByRole("button", { name: "Close alpha" }));
    expect(await screen.findByRole("heading", { name: "Getting started" })).toBeInTheDocument();
  });
});

describe("the Projects screen", () => {
  it("creates a project: picks a folder, the app prepares and opens it, and it is on screen", async () => {
    const user = userEvent.setup();
    const made: string[] = [];
    open("/projects", backend({
      restoreProjects: async () => restored([PA, A]),
      listProjects: async () => listOf([PA, "/work/new-one"], [PA, "/work/new-one"], PA),
      chooseFolder: async () => "/work/new-one",
      createProject: async (p) => {
        made.push(p);
        return { created: { path: p, made_folder: true, git_initialised: true }, connection: creds(B) };
      },
    }));
    await user.click(await screen.findByRole("button", { name: /Create New Project/ }));
    expect(await screen.findByText(/Created new-one: created the folder, set up a git repository, prepared it for a run, and opened it/)).toBeInTheDocument();
    expect(made).toEqual(["/work/new-one"]);
    expect(tab("new-one")).toHaveAttribute("aria-current", "true");
    // the new project's own server is what the app shows now
    await waitFor(() => expect(B.requests.some((r) => r.path === "/status")).toBe(true));
  });

  it("refuses to create over a folder that holds a run, with the reason", async () => {
    const user = userEvent.setup();
    open("/projects", backend({
      chooseFolder: async () => PB,
      createProject: async () => {
        throw new Error(`${PB} already holds a run (its .orchestrate/ has a plan or a state database): open it from the project list instead`);
      },
    }));
    await user.click(await screen.findByRole("button", { name: /Create New Project/ }));
    const alert = await screen.findByText(/already holds a run/);
    expect(alert).toHaveTextContent("open it from the project list");
    // nothing changed: still the two projects
    expect(within(tabs()).getAllByRole("listitem")).toHaveLength(3); // alpha, beta, + Project
  });

  it("cancelling the folder dialog does nothing", async () => {
    const user = userEvent.setup();
    const b = open("/projects", backend({ createProject: async () => { throw new Error("must not be called"); } }));
    await user.click(await screen.findByRole("button", { name: /Create New Project/ }));
    await settled();
    expect(screen.queryByText(/must not be called/)).not.toBeInTheDocument();
    expect(b.calls.some((c) => c.startsWith("open"))).toBe(false);
  });

  it("lists the open projects like the tabs do, and switching on a card switches the tab", async () => {
    const user = userEvent.setup();
    open("/projects");
    const beta = await screen.findByRole("article", { name: "beta" });
    const alpha = screen.getByRole("article", { name: "alpha" });
    expect(within(alpha).getByText("Active")).toBeInTheDocument();
    expect(tab("alpha")).toHaveAttribute("aria-current", "true");
    await user.click(within(beta).getByRole("button", { name: /Show this project/ }));
    await waitFor(() => expect(tab("beta")).toHaveAttribute("aria-current", "true"));
    expect(tab("alpha")).not.toHaveAttribute("aria-current");
    expect(within(await screen.findByRole("article", { name: "beta" })).getByText("Active")).toBeInTheDocument();
  });

  it("opens a listed project that is closed, shows it, and can close it again", async () => {
    const user = userEvent.setup();
    const b = open("/projects", backend({
      restoreProjects: async () => restored([PA, A]),
      listProjects: async () => listOf([PA, PB], [PA], PA),
    }));
    const card = await screen.findByRole("article", { name: "beta" });
    await user.click(within(card).getByRole("button", { name: "Open project" }));
    await waitFor(() => expect(b.calls).toContain(`open ${PB}`));
    expect(await within(tabs()).findByRole("button", { name: /^beta/ })).toHaveAttribute("aria-current", "true");
  });
});
