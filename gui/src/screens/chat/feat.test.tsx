import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import type { AgentKindInfo, ChatMessage, LeadView } from "../../api/types";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { setDefaultLeadLookupForTests } from "./defaultAgent";
import { bodyForTyped, featureWords, shownText, suggestsFeat } from "./featureCommand";

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
function open(o: { lead?: LeadView; run?: boolean } = {}) {
  if (o.run === false) server.respond("GET", "/chat", 404, errorBody("no_run", "no run"));
  else server.respond("GET", "/chat", 200, { messages: [] });
  server.respond("GET", "/lead", 200, o.lead ?? view({ state: "running", agent: "claude", status: "idle" }));
  server.respond("GET", "/agents", 200, { agents: [agent], team: [] });
  setDefaultLeadLookupForTests(async () => "claude");
  return renderApp({ route: "/chat", initial: server });
}
const box = () => screen.getByLabelText("Message to the lead") as HTMLTextAreaElement;
const posts = (path: string) => server.requests.filter((r) => r.method === "POST" && r.path === path);

describe("/orch feat: the command", () => {
  it("is recognised only as the command, with the free text after it", () => {
    expect(featureWords("/orch feat add dark mode")).toBe("add dark mode");
    expect(featureWords("  /ORCH  feat   spaced out ")).toBe("spaced out");
    expect(featureWords("/orch feat")).toBe("");
    expect(featureWords("/orch feature x")).toBeNull();
    expect(featureWords("hello /orch feat x")).toBeNull();
    expect(featureWords("what is /orch?")).toBeNull();
    expect(suggestsFeat("/")).toBe(true);
    expect(suggestsFeat("/or")).toBe(true);
    expect(suggestsFeat("/orch feat")).toBe(false);
    expect(suggestsFeat("/other")).toBe(false);
    expect(suggestsFeat("hi")).toBe(false);
    // plain text and a bare command go as typed; only the command with words is expanded
    expect(bodyForTyped("hello")).toEqual({ kind: "message", text: "hello", request: null });
    expect(bodyForTyped("/orch feat").text).toBe("/orch feat");
    const b = bodyForTyped("/orch feat add dark mode");
    expect(shownText(b.text)).toBe("/orch feat add dark mode");
    expect(shownText("an ordinary message")).toBe("an ordinary message");
  });

  it("(1)(2) a command with words goes to the Lead as a feature request in plain words, and the chat shows only what was typed", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByLabelText("Message to the lead");
    server.respond("POST", "/chat", 201, msg(1, { text: bodyForTyped("/orch feat add dark mode so night owls can read").text }));
    await user.type(box(), "/orch feat add dark mode so night owls can read{Enter}");
    await waitFor(() => expect(posts("/chat")).toHaveLength(1));
    const body = JSON.parse(posts("/chat")[0]!.body) as { kind: string; text: string; request: unknown };
    expect(body.kind).toBe("message");
    expect(body.request).toBeNull();
    expect(body.text.startsWith("/orch feat add dark mode so night owls can read")).toBe(true);
    // what the Lead is asked to do with it
    for (const w of ["Feature, Why, Must, Must not, Already decided, Done when, Mode", "ask me only for what you truly cannot infer", "Show me the structured request here in the chat", "wait for my yes before you plan"]) {
      expect(body.text).toContain(w);
    }
    // no field is asked of the developer
    expect(screen.queryByRole("dialog")).toBeNull();
    const log = screen.getByRole("log", { name: /Conversation with the lead/ });
    const mine = await within(log).findByText("/orch feat add dark mode so night owls can read");
    expect(mine.textContent).not.toContain("Turn it into the structured request");
  });

  it("(1) the command alone says in plain words to add a description, sends nothing and keeps the text", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByLabelText("Message to the lead");
    await user.type(box(), "/orch feat{Enter}");
    expect(await screen.findByText(/Add a description after \/orch feat/)).toBeInTheDocument();
    expect(posts("/chat")).toHaveLength(0);
    expect(posts("/lead/start")).toHaveLength(0);
    expect(box()).toHaveValue("/orch feat");
    // typing on clears the notice
    await user.type(box(), " add search");
    expect(screen.queryByText(/Add a description after/)).toBeNull();
  });

  it("(4) typing / offers /orch feat; choosing it (click, or Enter) completes the command instead of sending", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByLabelText("Message to the lead");
    expect(screen.queryByRole("listbox", { name: "Commands" })).toBeNull();
    await user.type(box(), "/");
    const list = await screen.findByRole("listbox", { name: "Commands" });
    await user.click(within(list).getByRole("button", { name: /\/orch feat/ }));
    expect(box()).toHaveValue("/orch feat ");
    expect(screen.queryByRole("listbox", { name: "Commands" })).toBeNull();
    expect(box()).toHaveFocus();
    // by keyboard: Enter on a command being typed completes it
    await user.clear(box());
    await user.type(box(), "/or{Enter}");
    expect(box()).toHaveValue("/orch feat ");
    expect(posts("/chat")).toHaveLength(0);
  });

  it("(5) with no Lead running (and no run) the command starts the Lead with the request in plain words", async () => {
    const user = userEvent.setup();
    open({ lead: view(), run: false });
    server.respond("POST", "/lead/start", 202, view({ state: "starting", agent: "claude" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Start Lead" })).toBeEnabled());
    await user.type(box(), "/orch feat add dark mode{Enter}");
    await waitFor(() => expect(posts("/lead/start")).toHaveLength(1));
    const sent = JSON.parse(posts("/lead/start")[0]!.body) as { agent: string; message: string };
    expect(sent.agent).toBe("claude");
    expect(sent.message.startsWith("/orch feat add dark mode")).toBe(true);
    expect(sent.message).toContain("Show me the structured request here in the chat");
    const log = screen.getByRole("log", { name: /Conversation with the lead/ });
    expect(await within(log).findByText("/orch feat add dark mode")).toBeInTheDocument();
  });

  it("(3) the New feature request form still works", async () => {
    const user = userEvent.setup();
    open();
    server.respond("POST", "/chat", 201, msg(2, { kind: "feature_request", text: "", request: { feature: "Dark mode", why: "night owls", must: "", must_not: "", decided: "", done_when: "", mode: "" } }));
    await user.click(await screen.findByRole("button", { name: "Open feature request form" }));
    await user.type(await screen.findByLabelText(/^Feature/), "Dark mode");
    await user.type(screen.getByLabelText(/^Why/), "night owls");
    await user.click(screen.getByRole("button", { name: "Send feature request" }));
    await waitFor(() => expect(posts("/chat")).toHaveLength(1));
    expect(JSON.parse(posts("/chat")[0]!.body)).toMatchObject({ kind: "feature_request", request: { feature: "Dark mode", why: "night owls" } });
  });
});
