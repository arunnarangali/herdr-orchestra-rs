import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import type { AgentKindInfo, ChatMessage, LeadView } from "../../api/types";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { setDefaultLeadLookupForTests } from "./defaultAgent";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  server.respond("GET", "/chat", 404, errorBody("no_run", "no run"));
  window.localStorage.clear();
});
afterEach(async () => {
  setDefaultLeadLookupForTests(undefined);
  await server.close();
});

const kindInfo = (kind: string, installed: boolean, display: string): AgentKindInfo => ({
  ...samples.agents.agents[0]!,
  kind,
  display_name: display,
  installed,
  roles: ["lead", "coder"],
});
const AGENTS = [kindInfo("claude", true, "Claude Code"), kindInfo("codex", true, "Codex CLI"), kindInfo("amp", false, "Amp")];
const view = (over: Partial<LeadView> = {}): LeadView => ({ state: "none", agent: null, pane: null, status: null, note: null, ...over });
const msg = (id: number, over: Partial<ChatMessage> = {}): ChatMessage => ({
  id,
  ts: "2026-11-15T09:30:00Z",
  sender: "developer",
  kind: "message",
  text: `message ${id}`,
  request: null,
  about: null,
  delivery: "queued",
  delivery_note: null,
  ...over,
});

function open(opts: { lead?: LeadView; agents?: AgentKindInfo[]; saved?: string | null } = {}) {
  server.respond("GET", "/lead", 200, opts.lead ?? view());
  server.respond("GET", "/agents", 200, { agents: opts.agents ?? AGENTS, team: [] });
  setDefaultLeadLookupForTests(async () => (opts.saved === undefined ? "claude" : opts.saved));
  return renderApp({ route: "/chat", initial: server });
}
const starts = () => server.requests.filter((r) => r.method === "POST" && r.path === "/lead/start");
const chats = () => server.requests.filter((r) => r.method === "POST" && r.path === "/chat");
const startButton = async () => {
  const b = await screen.findByRole("button", { name: "Start Lead" });
  await waitFor(() => expect(b).toBeEnabled()); // enabled once the installed agents are known
  return b;
};
const live = async (event: string, data: unknown, id = 1) => {
  await waitFor(() => expect(server.streamRequests().length).toBeGreaterThan(0));
  await act(async () => server.emit(id, event, data));
};

describe("Chat with no Lead", () => {
  it("AC1: offers Start Lead naming the project's default agent, and a double click starts once and shows Starting", async () => {
    const user = userEvent.setup();
    open();
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "claude" }));
    const button = await startButton();
    expect(await screen.findByText(/Will use/)).toHaveTextContent("Claude Code");
    expect(screen.getByText("No Lead is running")).toBeInTheDocument();
    expect(document.querySelector(".panel--error")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    await user.dblClick(button);
    await waitFor(() => expect(screen.getByText("Starting")).toBeInTheDocument());
    expect(starts()).toHaveLength(1);
    expect(JSON.parse(starts()[0]!.body)).toEqual({ agent: "claude", message: null });
    expect(screen.queryByRole("button", { name: "Start Lead" })).toBeNull();
    // still Starting until the state changes
    expect(screen.getByRole("button", { name: /Starting/ })).toBeDisabled();
  });

  it("AC4: when the stream says the Lead is running, the panel shows its agent and live status, and no Start Lead", async () => {
    const user = userEvent.setup();
    open();
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "claude" }));
    await user.click(await startButton());
    await live("lead", view({ state: "running", agent: "claude", status: "idle" }));
    expect(await screen.findByText("Lead running")).toBeInTheDocument();
    expect(screen.getByText("Idle")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Start Lead|Starting/ })).toBeNull();
    await live("lead", view({ state: "running", agent: "claude", status: "working" }), 2);
    expect(await screen.findByText("Working")).toBeInTheDocument();
  });

  it("AC2: any first message starts the Lead with that message, shows it as sent, and the Lead's reply lands in the same conversation", async () => {
    const user = userEvent.setup();
    open();
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "claude" }));
    await startButton();
    await user.type(screen.getByLabelText("Message to the lead"), "hi, what can you do?{Enter}");
    await waitFor(() => expect(starts()).toHaveLength(1));
    expect(JSON.parse(starts()[0]!.body)).toEqual({ agent: "claude", message: "hi, what can you do?" });
    expect(chats()).toHaveLength(0);
    const log = screen.getByRole("log", { name: /Conversation with the lead/ });
    const mine = await within(log).findByText("hi, what can you do?");
    expect(mine.closest("li")).toHaveTextContent("Sent");
    expect(screen.getByLabelText("Message to the lead")).toHaveValue("");
    expect(screen.queryByText(/Not sent/)).toBeNull();
    // the Lead is up and answers: its reply shows under the same message
    server.respond("GET", "/chat", 200, { messages: [msg(1, { sender: "lead", delivery: null, text: "I can plan features." })] });
    await live("lead", view({ state: "running", agent: "claude", status: "working" }));
    await live("chat", msg(1, { sender: "lead", delivery: null, text: "I can plan features." }), 2);
    expect(await within(log).findByText("I can plan features.")).toBeInTheDocument();
    expect(within(log).getByText("hi, what can you do?")).toBeInTheDocument();
  });

  it("AC5: with a run, the first message is stored through the chat route (not only kept in the browser) and the Lead starts without it", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/chat", 200, { messages: [] });
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "claude" }));
    server.respond("POST", "/chat", 201, msg(7, { text: "what is the plan?" }));
    open();
    await startButton();
    // the stored conversation has been read, so a run exists
    await screen.findByLabelText("Message to the lead");
    await user.type(screen.getByLabelText("Message to the lead"), "what is the plan?{Enter}");
    await waitFor(() => expect(chats()).toHaveLength(1));
    expect(JSON.parse(chats()[0]!.body)).toMatchObject({ kind: "message", text: "what is the plan?" });
    expect(starts()).toHaveLength(1);
    expect(JSON.parse(starts()[0]!.body)).toEqual({ agent: "claude", message: null });
    const log = screen.getByRole("log", { name: /Conversation with the lead/ });
    expect(await within(log).findByText("what is the plan?")).toBeInTheDocument();
    // nothing was kept only in the browser
    expect(window.localStorage.getItem("orch.chat.started:project")).toBeNull();
  });

  it("AC5: the first message that started the Lead is still there after the screen is opened again", async () => {
    const user = userEvent.setup();
    const first = open();
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "claude" }));
    await startButton();
    await user.type(screen.getByLabelText("Message to the lead"), "plan me a login page{Enter}");
    await screen.findByText("Passed to the Lead as it started.");
    first.unmount();
    server.respond("GET", "/lead", 200, view({ state: "running", agent: "claude", status: "working" }));
    renderApp({ route: "/chat", initial: server });
    expect(await screen.findByText("plan me a login page")).toBeInTheDocument();
    expect(await screen.findByText("Lead running")).toBeInTheDocument();
  });

  it("AC3: with no saved default it asks to choose an installed agent; a missing one is not offered", async () => {
    const user = userEvent.setup();
    open({ saved: null });
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "codex" }));
    const select = await screen.findByRole("combobox", { name: /Choose the agent/ });
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual([
      "Choose which agent starts the Lead…",
      "Claude Code (Installed)",
      "Codex CLI (Installed)",
    ]);
    expect(screen.getByRole("button", { name: "Start Lead" })).toBeDisabled();
    // a first message without a choice is not lost and says what is needed
    await user.type(screen.getByLabelText("Message to the lead"), "hello{Enter}");
    expect(await screen.findByText(/Choose which agent starts the Lead first/)).toBeInTheDocument();
    expect(starts()).toHaveLength(0);
    await user.selectOptions(select, "codex");
    await user.click(screen.getByRole("button", { name: "Start Lead" }));
    await waitFor(() => expect(starts()).toHaveLength(1));
    expect(JSON.parse(starts()[0]!.body).agent).toBe("codex");
  });

  it("AC3: a saved agent that is not installed asks for another", async () => {
    open({ saved: "amp" });
    expect(await screen.findByRole("combobox", { name: /saved agent \(amp\) is not installed/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start Lead" })).toBeDisabled();
  });

  it("AC3: with no installed agent it says so, points to Settings and offers no start", async () => {
    open({ agents: [kindInfo("amp", false, "Amp")], saved: null });
    expect(await screen.findByText(/No agent CLI that can lead is installed/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open Settings" })).toHaveAttribute("href", "/settings");
    expect(screen.queryByRole("button", { name: "Start Lead" })).toBeNull();
    expect(starts()).toHaveLength(0);
  });

  it("AC5: a failed start shows the server's words with a retry, which asks again", async () => {
    const user = userEvent.setup();
    open();
    server.respond("POST", "/lead/start", 503, errorBody("herdr_unavailable", "Herdr is not running and could not be started."));
    await user.click(await startButton());
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The Lead did not start.");
    expect(alert).toHaveTextContent("Herdr is not running and could not be started.");
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "claude" }));
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(starts()).toHaveLength(2));
    expect(await screen.findByText("Starting")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("AC5: a failed start from the first message is kept with its reason and can be retried", async () => {
    const user = userEvent.setup();
    open();
    server.respond("POST", "/lead/start", 400, errorBody("agent_not_installed", "claude is not installed."));
    await startButton(); // the Lead's state is known, so the message has to start it
    await user.type(await screen.findByLabelText("Message to the lead"), "hello{Enter}");
    expect(await screen.findByText("claude is not installed.", { selector: ".chat-note" })).toBeInTheDocument();
    expect(screen.getByLabelText("Message to the lead")).toHaveValue("hello");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("AC5: a Lead that disappears returns to the no-Lead state and keeps the stored conversation", async () => {
    open({ lead: view({ state: "running", agent: "claude", status: "idle" }) });
    server.respond("GET", "/chat", 200, { messages: [msg(1, { delivery: "acknowledged" }), msg(2, { sender: "lead", delivery: null, text: "done" })] });
    expect(await screen.findByText("Lead running")).toBeInTheDocument();
    await screen.findByText("message 1");
    await live("lead", view({ state: "gone", agent: "claude", note: "The Lead's pane closed." }));
    expect(await screen.findByText("The Lead has stopped")).toBeInTheDocument();
    expect(screen.getByText("The Lead's pane closed.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start Lead" })).toBeEnabled();
    expect(screen.getByText("message 1")).toBeInTheDocument();
    expect(screen.getByText("done")).toBeInTheDocument();
  });
});

describe("Chat with a Lead running", () => {
  it("AC4: shows the agent and its status, the messages, a ready input, and sends into the same conversation", async () => {
    const user = userEvent.setup();
    open({ lead: view({ state: "running", agent: "claude", status: "working" }) });
    server.respond("GET", "/chat", 200, { messages: [msg(1, { delivery: "acknowledged" })] });
    server.respond("POST", "/chat", 201, msg(2, { text: "next step?" }));
    expect(await screen.findByText("Lead running")).toBeInTheDocument();
    expect(await screen.findByText(/with Claude Code/)).toBeInTheDocument();
    expect(screen.getByText("Working")).toBeInTheDocument();
    expect(await screen.findByText("message 1")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Start Lead|Starting/ })).toBeNull();
    const box = screen.getByLabelText("Message to the lead");
    expect(box).toBeEnabled();
    await user.type(box, "next step?{Enter}");
    await waitFor(() => expect(chats()).toHaveLength(1));
    expect(starts()).toHaveLength(0);
    await live("lead", view({ state: "running", agent: "claude", status: "blocked" }));
    expect(await screen.findByText("Blocked")).toBeInTheDocument();
  });

  it("AC4: with the Lead running before any run exists, a message is held as waiting and sent by itself once the run is there", async () => {
    const user = userEvent.setup();
    open({ lead: view({ state: "running", agent: "claude", status: "working" }) });
    server.respond("GET", "/health", 200, { ...samples.health, run: false });
    server.respond("POST", "/chat", 404, errorBody("no_run", "no run exists"));
    expect(await screen.findByText("Lead running")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Message to the lead"), "keep me posted{Enter}");
    expect(await screen.findByText(/Waiting: it is sent as soon as/)).toBeInTheDocument();
    expect(screen.queryByText(/Not sent/)).toBeNull();
    expect(screen.getByLabelText("Message to the lead")).toHaveValue("");
    // the Lead sets up the run: the held message goes out without another press
    server.respond("GET", "/health", 200, { ...samples.health, run: true });
    server.respond("GET", "/chat", 200, { messages: [] });
    server.respond("POST", "/chat", 201, msg(1, { text: "keep me posted" }));
    await waitFor(() => expect(chats().length).toBeGreaterThan(1));
    await waitFor(() => expect(screen.queryByText(/Waiting: it is sent/)).toBeNull());
    expect(await screen.findByText("keep me posted")).toBeInTheDocument();
  });

  it("AC6: a project with no run never shows the old 'Not sent: no run yet' message in the normal flow", async () => {
    open();
    expect(await screen.findByText("No run in this project yet.")).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Not sent: this project has no run yet");
    expect(document.body).not.toHaveTextContent("orch lead plan");
  });
});
