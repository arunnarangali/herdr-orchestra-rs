import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { ChatMessage } from "../../api/types";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
});
afterEach(() => server.close());

function msg(id: number, over: Partial<ChatMessage> = {}): ChatMessage {
  return {
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
  };
}
const lead = (id: number, over: Partial<ChatMessage> = {}) =>
  msg(id, { sender: "lead", delivery: null, text: `reply ${id}`, ...over });

const open = (messages: ChatMessage[] = []) => {
  server.respond("GET", "/chat", 200, { messages });
  return renderApp({ route: "/chat", initial: server });
};
const posts = () => server.requests.filter((r) => r.method === "POST" && r.path === "/chat");
const bodyOf = (i = 0) => JSON.parse(posts()[i]!.body) as Record<string, unknown>;
const stored = (m: ChatMessage) => server.respond("POST", "/chat", 201, m);
const FULL = { feature: "Dark mode", why: "night owls" };

describe("the conversation", () => {
  it("shows an empty state, then the messages in order with the developer's and the lead's apart", async () => {
    open([msg(1), lead(2, { about: "M2" }), msg(3, { delivery: "acknowledged" })]);
    const log = await screen.findByRole("log", { name: /Conversation with the lead/ });
    const rows = within(log).getAllByRole("listitem").filter((li) => li.getAttribute("data-message"));
    expect(rows.map((r) => r.getAttribute("data-message"))).toEqual(["1", "2", "3"]);
    expect(rows[0]).toHaveClass("chat-row--mine");
    expect(rows[1]).toHaveClass("chat-row--lead");
    expect(rows[1]).toHaveTextContent("about M2");
    expect(rows[1]).toHaveTextContent("reply 2");
  });

  it("says so when there is nothing yet, and while it loads", async () => {
    open([]);
    expect(screen.getByText(/Loading the conversation/)).toBeInTheDocument();
    expect(await screen.findByText(/Nothing here yet/)).toBeInTheDocument();
  });

  it("explains a failed read and can try again", async () => {
    server.respond("GET", "/chat", 404, errorBody("no_run", "no run"));
    renderApp({ route: "/chat", initial: server });
    expect(await screen.findByText("This project has no run yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("updates live from the chat stream event: a new reply and a delivery change", async () => {
    open([msg(1)]);
    await screen.findByText("message 1");
    expect(screen.getByText("Queued")).toBeInTheDocument();
    await waitFor(() => expect(server.streamRequests().length).toBeGreaterThan(0));
    await act(async () => {
      server.emit(1, "chat", msg(1, { delivery: "sent" }));
      server.emit(2, "chat", lead(2, { text: "on it" }));
    });
    expect(await screen.findByText("on it")).toBeInTheDocument();
    expect(await screen.findByText("Sent")).toBeInTheDocument();
    expect(screen.queryByText("Queued")).not.toBeInTheDocument();
    // still one bubble for message 1
    expect(document.querySelectorAll('[data-message="1"]')).toHaveLength(1);
  });

  it("scrolls to the newest message", async () => {
    const scrolled: Element[] = [];
    Element.prototype.scrollIntoView = function (this: Element) {
      scrolled.push(this);
    };
    open([msg(1)]);
    await screen.findByText("message 1");
    await waitFor(() => expect(scrolled.length).toBeGreaterThan(0));
    await waitFor(() => expect(server.streamRequests().length).toBeGreaterThan(0));
    const before = scrolled.length;
    await act(async () => server.emit(1, "chat", lead(2)));
    await screen.findByText("reply 2");
    await waitFor(() => expect(scrolled.length).toBeGreaterThan(before));
    expect(scrolled.at(-1)).toHaveClass("chat-end");
    // @ts-expect-error cleaning up the jsdom gap
    delete Element.prototype.scrollIntoView;
  });
});

describe("sending", () => {
  it("sends with the keyboard (Enter), shows the message with its state, and clears the box", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText(/Nothing here yet/);
    stored(msg(1, { text: "hello lead" }));
    await user.type(screen.getByLabelText("Message to the lead"), "hello lead{Enter}");
    expect(await screen.findByText("hello lead")).toBeInTheDocument();
    expect(await screen.findByText("Queued")).toBeInTheDocument();
    expect(bodyOf()).toEqual({ kind: "message", text: "hello lead", request: null });
    expect(screen.getByLabelText("Message to the lead")).toHaveValue("");
  });

  it("Shift+Enter makes a new line instead of sending, and an empty box sends nothing", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText(/Nothing here yet/);
    const box = screen.getByLabelText("Message to the lead");
    await user.type(box, "a{Shift>}{Enter}{/Shift}b");
    expect(box).toHaveValue("a\nb");
    expect(posts()).toHaveLength(0);
    await user.clear(box);
    await user.type(box, "   {Enter}");
    expect(posts()).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("shows each delivery state in words: queued, sent, acknowledged, failed with its note", async () => {
    open([
      msg(1, { delivery: "queued", delivery_note: "waiting: the lead is working" }),
      msg(2, { delivery: "sent" }),
      msg(3, { delivery: "acknowledged" }),
      msg(4, { delivery: "failed", delivery_note: "could not be typed into the lead's pane: send the message again" }),
    ]);
    await screen.findByText("message 4");
    const state = (id: number) => document.querySelector(`[data-message="${id}"] [data-delivery]`)!;
    expect(state(1)).toHaveTextContent("Queued");
    expect(state(1)).toHaveTextContent("waiting: the lead is working");
    expect(state(2)).toHaveTextContent("Sent");
    expect(state(3)).toHaveTextContent("Acknowledged");
    expect(state(4)).toHaveTextContent("Failed");
    expect(state(4)).toHaveTextContent("send the message again");
    expect(within(state(1) as HTMLElement).queryByRole("button")).toBeNull();
  });

  it("a failed message can be retried: the same text is sent again", async () => {
    const user = userEvent.setup();
    open([msg(4, { text: "ship it", delivery: "failed", delivery_note: "try again" })]);
    stored(msg(5, { text: "ship it" }));
    await user.click(await screen.findByRole("button", { name: "Retry" }));
    expect(await screen.findAllByText("ship it")).toHaveLength(2);
    expect(bodyOf()).toEqual({ kind: "message", text: "ship it", request: null });
  });

  it("a send that never reached the server stays as 'Not sent' with the reason, and Retry sends it", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText(/Nothing here yet/);
    server.respond("POST", "/chat", 500, errorBody("internal", "internal error"));
    await user.type(screen.getByLabelText("Message to the lead"), "important{Enter}");
    const failed = await screen.findByText("Not sent");
    expect(failed.closest("li")).toHaveTextContent("internal error");
    expect(failed.closest("li")).toHaveTextContent("important");

    stored(msg(1, { text: "important" }));
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.queryByText("Not sent")).not.toBeInTheDocument());
    expect(await screen.findByText("Queued")).toBeInTheDocument();
    expect(posts()).toHaveLength(2);
  });

  it("with no lead it says so in plain words and how to start one, and keeps the typed text", async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText(/Nothing here yet/);
    server.respond(
      "POST",
      "/chat",
      409,
      errorBody("lead_unavailable", "no lead pane exists: Herdr has no pane named `lead`. (message #1 is stored)"),
    );
    // the message was stored all the same, so the next read has it
    server.respond("GET", "/chat", 200, { messages: [msg(1, { text: "anyone there?" })] });
    await user.type(screen.getByLabelText("Message to the lead"), "anyone there?{Enter}");
    const alert = await screen.findByText("The lead is not running");
    expect(alert.closest("[role=alert]")).toHaveTextContent("orch lead plan");
    expect(alert.closest("[role=alert]")).toHaveTextContent("no pane named `lead`");
    expect(screen.getByLabelText("Message to the lead")).toHaveValue("anyone there?");
    expect(await screen.findByText("Queued")).toBeInTheDocument();
  });
});

describe("the feature request form", () => {
  const openForm = async () => {
    const user = userEvent.setup();
    open();
    await screen.findByText(/Nothing here yet/);
    await user.click(screen.getByRole("button", { name: "New feature request" }));
    return user;
  };

  it("has every field of the plan's request, with Feature and Why marked required", async () => {
    await openForm();
    const form = screen.getByRole("form", { name: "New feature request" });
    for (const label of [/^Feature/, /^Why/, "Must", "Must not", "Already decided", "Done when", "Mode"]) {
      expect(within(form).getByLabelText(label)).toBeInTheDocument();
    }
    expect(within(form).getByLabelText(/^Feature/)).toBeRequired();
    expect(within(form).getByLabelText(/^Why/)).toBeRequired();
    expect(within(form).getByLabelText("Must")).not.toBeRequired();
  });

  it("refuses an empty Feature or Why, says which, focuses it and sends nothing", async () => {
    const user = await openForm();
    await user.click(screen.getByRole("button", { name: "Send feature request" }));
    expect(screen.getByText("Say what the feature is.")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Feature/)).toHaveFocus();
    expect(screen.getByLabelText(/^Feature/)).toHaveAttribute("aria-invalid", "true");
    await user.type(screen.getByLabelText(/^Feature/), "Dark mode");
    await user.click(screen.getByRole("button", { name: "Send feature request" }));
    expect(screen.queryByText("Say what the feature is.")).not.toBeInTheDocument();
    expect(screen.getByText("Say why it is wanted.")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Why/)).toHaveFocus();
    expect(posts()).toHaveLength(0);
  });

  it("sends a feature_request with the fields as separate values, then shows it in the conversation", async () => {
    const user = await openForm();
    const request = { ...FULL, must: "persist", must_not: "touch the API", decided: "CSS variables", done_when: "toggle works", mode: "worktree" };
    stored(msg(1, { kind: "feature_request", text: "Feature request: Dark mode", request }));
    await user.type(screen.getByLabelText(/^Feature/), request.feature);
    await user.type(screen.getByLabelText(/^Why/), request.why);
    await user.type(screen.getByLabelText("Must"), request.must);
    await user.type(screen.getByLabelText("Must not"), request.must_not);
    await user.type(screen.getByLabelText("Already decided"), request.decided);
    await user.type(screen.getByLabelText("Done when"), request.done_when);
    await user.selectOptions(screen.getByLabelText("Mode"), "worktree");
    await user.click(screen.getByRole("button", { name: "Send feature request" }));
    await waitFor(() => expect(screen.queryByRole("form", { name: "New feature request" })).not.toBeInTheDocument());
    expect(bodyOf()).toEqual({ kind: "feature_request", text: "", request });
    const row = document.querySelector('[data-message="1"]') as HTMLElement;
    expect(row).toHaveTextContent("Feature request");
    expect(row).toHaveTextContent("Dark mode");
    expect(row).toHaveTextContent("touch the API");
    expect(row).toHaveTextContent("Queued");
  });

  it("with no lead it says so, how to start one, and keeps everything typed", async () => {
    const user = await openForm();
    server.respond("POST", "/chat", 409, errorBody("lead_unavailable", "no lead pane exists: this run has no lead."));
    await user.type(screen.getByLabelText(/^Feature/), FULL.feature);
    await user.type(screen.getByLabelText(/^Why/), FULL.why);
    await user.type(screen.getByLabelText("Must"), "keep it small");
    await user.click(screen.getByRole("button", { name: "Send feature request" }));
    const alert = (await screen.findByText("The lead is not running")).closest("[role=alert]")!;
    expect(alert).toHaveTextContent("orch lead plan");
    expect(screen.getByLabelText(/^Feature/)).toHaveValue(FULL.feature);
    expect(screen.getByLabelText(/^Why/)).toHaveValue(FULL.why);
    expect(screen.getByLabelText("Must")).toHaveValue("keep it small");
    // closing and reopening keeps the draft too
    await user.click(screen.getByRole("button", { name: "Close" }));
    await user.click(screen.getByRole("button", { name: "New feature request" }));
    expect(screen.getByLabelText(/^Feature/)).toHaveValue(FULL.feature);
  });

  it("shows another refusal as it is and keeps the text", async () => {
    const user = await openForm();
    server.respond("POST", "/chat", 400, errorBody("bad_request", "`request.feature` is too long"));
    await user.type(screen.getByLabelText(/^Feature/), FULL.feature);
    await user.type(screen.getByLabelText(/^Why/), FULL.why);
    await user.click(screen.getByRole("button", { name: "Send feature request" }));
    expect(await screen.findByText("`request.feature` is too long")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Feature/)).toHaveValue(FULL.feature);
  });
});

describe("the Chat screen in the app", () => {
  it("opens from the sidebar and with g then c", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/chat", 200, { messages: [] });
    renderApp({ route: "/overview", initial: server });
    await user.keyboard("gc");
    expect(await screen.findByRole("heading", { name: "Chat" })).toBeInTheDocument();
  });
});
