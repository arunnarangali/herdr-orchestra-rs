import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { deadAddress, startFakeServer, type FakeServer } from "../test/fakeServer";
import { fakeShell, renderApp } from "../test/render";

let server: FakeServer;
const open: FakeServer[] = [];
beforeEach(async () => {
  server = await startFakeServer();
  open.push(server);
});
afterEach(async () => {
  await Promise.all(open.splice(0).map((s) => s.close()));
});

const pill = () => screen.getByRole("status", { name: /^Connection:/ });

describe("connection state in the app frame", () => {
  it("connected: shows the project and no warning", async () => {
    renderApp({ route: "/workflows", initial: server });
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Connected"));
    expect(pill()).toHaveAttribute("title", "/home/me/project");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
  });

  it("no server: says so and what to do, and keeps trying", async () => {
    const url = await deadAddress();
    renderApp({ route: "/workflows", initial: { url, token: server.token } });
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: No server"));
    const banner = screen.getByRole("alert");
    expect(banner).toHaveTextContent(`No server answers at ${url}`);
    expect(banner).toHaveTextContent("orch serve");
    expect(within(banner).getByRole("button", { name: "Check again" })).toBeInTheDocument();
  });

  it("bad token: says the token was refused and never shows it", async () => {
    renderApp({ route: "/workflows", initial: { url: server.url, token: "x".repeat(64) } });
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Bad token"));
    const banner = screen.getByRole("alert");
    expect(banner).toHaveTextContent("The server refused the token");
    expect(banner).toHaveTextContent("api-token");
    expect(document.body).not.toHaveTextContent("x".repeat(64));
  });

  it("no run: the server works but the project has no run", async () => {
    const noRun = await startFakeServer({ run: false });
    open.push(noRun);
    renderApp({ route: "/workflows", initial: noRun });
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: No run"));
    expect(screen.getByRole("alert")).toHaveTextContent("orch sync");
    expect(noRun.streamRequests()).toHaveLength(0);
  });

  it("no project chosen: shows Not connected", () => {
    renderApp({ route: "/overview" });
    expect(pill()).toHaveAccessibleName("Connection: Not connected");
  });

  it("a stream that is refused after a good health check is a bad token", async () => {
    const odd = await startFakeServer();
    open.push(odd);
    // Health passes, then the token is replaced: the stream is refused.
    renderApp({ route: "/workflows", initial: odd });
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Connected"));
    odd.rejectToken();
    odd.dropStreams();
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Bad token"));
  });

  it("reconnects: server goes away (no server), comes back (connected, stream resumes with Last-Event-ID)", async () => {
    renderApp({ route: "/workflows", initial: server });
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Connected"));
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    server.emit(9, "run-state", { run_control: "paused" });
    // let the message arrive before the server disappears
    await new Promise((r) => setTimeout(r, 60));

    const { port, token } = server;
    await server.close();
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: No server"));

    const back = await startFakeServer({ port, token });
    open.push(back);
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Connected"));
    await waitFor(() => expect(back.streamRequests()).toHaveLength(1));
    expect(back.streamRequests()[0]?.headers["last-event-id"]).toBe("9");
  });
});

describe("choosing a server", () => {
  it("browser: the form takes a URL and token, connects, and leaves the connect step", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/projects" });
    // No project yet: redirected to Getting started, with the form.
    expect(await screen.findByRole("heading", { name: "Getting started" })).toBeInTheDocument();
    const token = screen.getByLabelText("Token");
    expect(token).toHaveAttribute("type", "password");

    await user.clear(screen.getByLabelText("Server address"));
    await user.type(screen.getByLabelText("Server address"), server.url);
    await user.type(token, server.token);
    await user.click(screen.getByRole("button", { name: "Connect" }));

    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Connected"));
    expect(await screen.findByRole("heading", { name: "Overview" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Token")).not.toBeInTheDocument();
    // Kept in memory only.
    expect(JSON.stringify({ ...window.localStorage })).not.toContain(server.token);
    expect(window.sessionStorage.length).toBe(0);
  });

  it("browser: asks for a valid address and a token", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview" });
    await user.clear(screen.getByLabelText("Server address"));
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter the server address");
    await user.type(screen.getByLabelText("Server address"), "127.0.0.1:1");
    await user.click(screen.getByRole("button", { name: "Connect" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter the token");
  });

  it("desktop shell: choosing a folder starts the server and connects with the URL and token it hands over", async () => {
    const user = userEvent.setup();
    const started: string[] = [];
    const shell = fakeShell({
      chooseProjectFolder: async () => "/work/proj",
      startServer: async (p) => {
        started.push(p);
        return { url: server.url, token: server.token };
      },
    });
    renderApp({ route: "/overview", shell });
    await user.click(await screen.findByRole("button", { name: "Choose project folder…" }));
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Connected"));
    expect(started).toEqual(["/work/proj"]);
  });

  it("desktop shell: shows the shell's message when orch is missing", async () => {
    const user = userEvent.setup();
    const shell = fakeShell({
      chooseProjectFolder: async () => "/work/proj",
      startServer: async () => {
        throw new Error("orch was not found: set ORCH_BIN or put it on your PATH");
      },
    });
    renderApp({ route: "/overview", shell });
    await user.click(await screen.findByRole("button", { name: "Choose project folder…" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("set ORCH_BIN or put it on your PATH");
    expect(pill()).toHaveAccessibleName("Connection: Not connected");
  });

  it("desktop shell: cancelling the folder dialog changes nothing", async () => {
    const user = userEvent.setup();
    renderApp({ route: "/overview", shell: fakeShell() });
    await user.click(await screen.findByRole("button", { name: "Choose project folder…" }));
    expect(pill()).toHaveAccessibleName("Connection: Not connected");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("desktop shell: picks up a server the shell already started", async () => {
    const shell = fakeShell({ getConnection: async () => ({ url: server.url, token: server.token }) });
    renderApp({ route: "/overview", shell });
    await waitFor(() => expect(pill()).toHaveAccessibleName("Connection: Connected"));
  });

  it("changing the connection stops the shell's server and shows the connect step again", async () => {
    const user = userEvent.setup();
    let stopped = 0;
    const shell = fakeShell({ stopServer: async () => void stopped++ });
    const url = await deadAddress();
    renderApp({ route: "/workflows", initial: { url, token: "t" }, shell });
    await user.click(await screen.findByRole("button", { name: "Change connection" }));
    expect(stopped).toBe(1);
    expect(await screen.findByRole("heading", { name: "Getting started" })).toBeInTheDocument();
  });
});
