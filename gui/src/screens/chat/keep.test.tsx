import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import type { AgentKindInfo, ChatMessage, LeadView } from "../../api/types";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { setDefaultLeadLookupForTests } from "./defaultAgent";

// Leaving Chat / Home and coming back shows the same conversation and the unsent text.

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  window.localStorage.clear();
});
afterEach(async () => {
  setDefaultLeadLookupForTests(undefined);
  await server.close();
});

const agent: AgentKindInfo = { ...samples.agents.agents[0]!, kind: "claude", display_name: "Claude Code", installed: true, roles: ["lead", "coder"] };
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

function open(o: { lead?: LeadView; messages?: ChatMessage[] | null } = {}) {
  if (o.messages === null) server.respond("GET", "/chat", 404, errorBody("no_run", "no run"));
  else server.respond("GET", "/chat", 200, { messages: o.messages ?? [] });
  server.respond("GET", "/lead", 200, o.lead ?? view());
  server.respond("GET", "/agents", 200, { agents: [agent], team: [] });
  setDefaultLeadLookupForTests(async () => "claude");
  return renderApp({ route: "/chat", initial: server });
}
const box = () => screen.getByLabelText("Message to the lead") as HTMLTextAreaElement;
const log = () => screen.getByRole("log", { name: /Conversation with the lead/ });
const leaveAndCome = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(await screen.findByRole("link", { name: "Settings" }));
  await screen.findByRole("heading", { name: "Settings" });
  expect(screen.queryByLabelText("Message to the lead")).toBeNull();
  await user.click(screen.getByRole("link", { name: /Chat \/ Home/ }));
  await screen.findByLabelText("Message to the lead");
};

describe("Chat keeps its conversation when the developer leaves the page", () => {
  it("no Lead, no run: the unsent text is still in the input on return", async () => {
    const user = userEvent.setup();
    open({ messages: null });
    await screen.findByLabelText("Message to the lead");
    await user.type(box(), "half a thought");
    await leaveAndCome(user);
    expect(box()).toHaveValue("half a thought");
    expect(await screen.findByRole("button", { name: "Start Lead" })).toBeInTheDocument();
  });

  it("with a run: every message and its state is shown again, and so is the unsent text", async () => {
    const user = userEvent.setup();
    open({
      lead: view({ state: "running", agent: "claude", status: "idle" }),
      messages: [msg(1, { text: "first", delivery: "sent" }), msg(2, { sender: "lead", delivery: null, text: "an answer" }), msg(3, { text: "third", delivery: "failed", delivery_note: "no pane" })],
    });
    await within(await screen.findByRole("log")).findByText("an answer");
    server.respond("POST", "/chat", 201, msg(4, { text: "just now" }));
    await user.type(box(), "just now{Enter}");
    await within(log()).findByText("just now");
    await user.type(box(), "next one");
    await leaveAndCome(user);
    const l = log();
    for (const t of ["first", "an answer", "third", "just now"]) expect(await within(l).findByText(t)).toBeInTheDocument();
    expect(within(l).getByText("third").closest("li")).toHaveTextContent("Failed");
    expect(within(l).getByText("just now").closest("li")).toHaveTextContent("Queued");
    expect(box()).toHaveValue("next one");
    expect(await screen.findByText("Lead running")).toBeInTheDocument();
  });

  it("with a Lead starting: the conversation and the Starting state are shown again on return", async () => {
    const user = userEvent.setup();
    open({ lead: view({ state: "starting", agent: "claude" }), messages: [msg(1, { text: "earlier", delivery: "sent" }), msg(2, { text: "queued one" })] });
    await within(await screen.findByRole("log")).findByText("queued one");
    await leaveAndCome(user);
    expect(await within(log()).findByText("queued one")).toBeInTheDocument();
    expect(within(log()).getByText("earlier").closest("li")).toHaveTextContent("Sent");
    expect((await screen.findAllByText(/Starting/)).length).toBeGreaterThan(0);
  });

  it("no run yet, Lead running: a held message ('Waiting') is still there on return", async () => {
    const user = userEvent.setup();
    open({ lead: view({ state: "running", agent: "claude", status: "idle" }), messages: null });
    await screen.findByText(/No run in this project yet/);
    server.respond("POST", "/chat", 404, errorBody("no_run", "no run exists in this project"));
    await user.type(box(), "plan a login page{Enter}");
    const held = await within(log()).findByText("plan a login page");
    expect(held.closest("li")).toHaveTextContent(/Waiting/);
    await leaveAndCome(user);
    const back = await within(log()).findByText("plan a login page");
    expect(back.closest("li")).toHaveTextContent(/Waiting/);
    // and it is sent by itself once the run exists
    server.respond("POST", "/chat", 201, msg(1, { text: "plan a login page" }));
    server.respond("GET", "/chat", 200, { messages: [] });
    await waitFor(() => expect(server.requests.filter((r) => r.method === "POST" && r.path === "/chat").length).toBeGreaterThanOrEqual(1));
  });

  it("the first message that starts the Lead (no run) is still shown as sent on return", async () => {
    const user = userEvent.setup();
    open({ messages: null });
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "claude" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Start Lead" })).toBeEnabled());
    await user.type(box(), "hello, who are you?{Enter}");
    const mine = await within(log()).findByText("hello, who are you?");
    expect(mine.closest("li")).toHaveTextContent("Sent");
    await leaveAndCome(user);
    const back = await within(log()).findByText("hello, who are you?");
    expect(back.closest("li")).toHaveTextContent("Sent");
    expect(box()).toHaveValue("");
  });
});
