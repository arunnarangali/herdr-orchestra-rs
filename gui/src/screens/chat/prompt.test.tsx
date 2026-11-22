import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import type { AgentPrompt, AttentionItem, LeadView } from "../../api/types";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { setBackendForTests } from "../projects/backend";
import { setDefaultLeadLookupForTests } from "./defaultAgent";
import { promptTiming } from "../../components/PromptAnswer";
import { attentionTiming } from "../../components/attentionStore";

const timing = { ...promptTiming };
const pollEvery = attentionTiming.promptPollMs;

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
  server.respond("GET", "/chat", 404, errorBody("no_run", "no run"));
  server.respond("GET", "/agents", 200, samples.agents);
  setDefaultLeadLookupForTests(async () => "claude");
  setBackendForTests(null);
  window.localStorage.clear();
  // re-read quickly after an answer, but never reach "still at this question" unless a test asks
  // for it: a loaded machine must not move a card past the state a test is looking at
  promptTiming.recheckMs = 50;
  promptTiming.stillWaitingMs = 60_000;
});
afterEach(async () => {
  Object.assign(promptTiming, timing);
  attentionTiming.promptPollMs = pollEvery;
  setDefaultLeadLookupForTests(undefined);
  setBackendForTests(undefined);
  await server.close();
});

const TRUST: AgentPrompt = {
  text: "Do you trust the files in this folder?\n\n/home/dev/project",
  options: [
    { id: "1-5d1f7c2a", label: "Yes, proceed" },
    { id: "2-5d1f7c2a", label: "No, exit" },
  ],
};
const lead = (over: Partial<LeadView> = {}): LeadView => ({
  state: "running",
  agent: "claude",
  pane: "w2:p1",
  status: "blocked",
  note: null,
  prompt: TRUST,
  ...over,
});
const blocked = (over: Partial<AttentionItem> = {}): AttentionItem => ({
  id: "blocked:1",
  kind: "blocked",
  task: "M1",
  agent: "rs-claude-1",
  title: "rs-claude-1 is blocked on permission",
  detail: "needs bash permission",
  prompt: { text: "Allow this command?\n\ncargo test", options: [{ id: "1-aa", label: "Yes" }, { id: "2-aa", label: "No" }] },
  since: "2026-11-22T10:00:00Z",
  actions: ["open_chat"],
  ...over,
});

// `blocked:1` as the client sends it (the id is URL-encoded in the path)
const PROMPT_PATH = "/attention/blocked%3A1/prompt";
const posts = (path: string) => server.requests.filter((r) => r.method === "POST" && r.path === path);
const live = async (event: string, data: unknown, id = 1) => {
  await waitFor(() => expect(server.streamRequests().length).toBeGreaterThan(0));
  await act(async () => server.emit(id, event, data));
};
const leadCard = () => screen.findByRole("group", { name: "Question from the Lead" });
const workerCard = async () => {
  const inbox = await screen.findByRole("list", { name: "Waiting items" });
  return within(inbox).findByRole("group", { name: "Question from rs-claude-1" });
};

describe("the Lead's prompt in Chat", () => {
  it("AC1/AC2: shows the question and one button per choice; a double click answers once, shows Sending, and the card goes when the stream says so", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/lead", 200, lead());
    server.respond("POST", "/lead/prompt", 200, lead());
    // the answer stays in flight until the test has looked at the Sending state
    const release = server.hold("POST", "/lead/prompt");
    renderApp({ route: "/chat", initial: server });

    const card = await leadCard();
    expect(screen.getByRole("heading", { name: "The Lead is waiting for your answer" })).toBeInTheDocument();
    expect(card).toHaveTextContent("Do you trust the files in this folder?");
    expect(card).toHaveTextContent("/home/dev/project");
    const buttons = within(card).getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual(["Yes, proceed", "No, exit"]);

    await user.dblClick(within(card).getByRole("button", { name: "Yes, proceed" }));
    expect(await within(card).findByRole("button", { name: "Sending “Yes, proceed”…" })).toBeDisabled();
    expect(within(card).getByRole("button", { name: "No, exit" })).toBeDisabled();
    expect(within(card).getByRole("status")).toHaveTextContent("Sending your answer to the Lead");
    await waitFor(() => expect(posts("/lead/prompt")).toHaveLength(1));
    release();

    await waitFor(() => expect(within(card).getByRole("status")).toHaveTextContent("Sent “Yes, proceed”. Waiting for the Lead to carry on"));
    expect(posts("/lead/prompt")).toHaveLength(1);
    expect(JSON.parse(posts("/lead/prompt")[0]!.body)).toEqual({ option: "1-5d1f7c2a" });
    // still waiting at the prompt: no second answer can be sent yet
    expect(within(card).getByRole("button", { name: "No, exit" })).toBeDisabled();

    await live("lead", lead({ status: "working", prompt: null }));
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from the Lead" })).toBeNull());
    expect(posts("/lead/prompt")).toHaveLength(1);
  });

  it("AC3: a refused answer shows the server's words with Try again, and the card stays", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/lead", 200, lead());
    server.respond("POST", "/lead/prompt", 503, errorBody("herdr_unavailable", "Herdr could not type the answer into the Lead's pane: try again"));
    renderApp({ route: "/chat", initial: server });

    const card = await leadCard();
    await user.click(within(card).getByRole("button", { name: "No, exit" }));
    const alert = await within(card).findByRole("alert");
    expect(alert).toHaveTextContent("“No, exit” was not sent.");
    expect(alert).toHaveTextContent("Herdr could not type the answer into the Lead's pane: try again");
    expect(within(card).getByRole("button", { name: "Yes, proceed" })).toBeEnabled();

    server.respond("POST", "/lead/prompt", 200, lead({ status: "working", prompt: null }));
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(posts("/lead/prompt")).toHaveLength(2));
    expect(JSON.parse(posts("/lead/prompt")[1]!.body)).toEqual({ option: "2-5d1f7c2a" });
    // the answer said the prompt is gone
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from the Lead" })).toBeNull());
  });

  it("AC4: a choice can be reached with Tab and pressed with Enter", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/lead", 200, lead());
    server.respond("POST", "/lead/prompt", 200, lead());
    renderApp({ route: "/chat", initial: server });

    const card = await leadCard();
    const yes = within(card).getByRole("button", { name: "Yes, proceed" });
    yes.focus();
    await user.tab();
    expect(within(card).getByRole("button", { name: "No, exit" })).toHaveFocus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(posts("/lead/prompt")).toHaveLength(1));
    expect(JSON.parse(posts("/lead/prompt")[0]!.body)).toEqual({ option: "2-5d1f7c2a" });
  });

  it("AC1: shows the first-run question while the Lead is starting, and answers it", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/lead", 200, lead({ state: "starting", status: "blocked", pane: "w2:p1", note: "starting the Lead" }));
    server.respond("POST", "/lead/prompt", 200, lead({ state: "starting", status: null, prompt: null }));
    renderApp({ route: "/chat", initial: server });

    const card = await leadCard();
    expect(card).toHaveTextContent("Do you trust the files in this folder?");
    expect(screen.getByText("Starting")).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Yes, proceed" }));
    await waitFor(() => expect(posts("/lead/prompt")).toHaveLength(1));
    expect(JSON.parse(posts("/lead/prompt")[0]!.body)).toEqual({ option: "1-5d1f7c2a" });
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from the Lead" })).toBeNull());
  });

  it("AC2: after an answer it reads the Lead again; only a read that still shows the question lets the developer choose again", async () => {
    promptTiming.stillWaitingMs = 300;
    const user = userEvent.setup();
    server.respond("GET", "/lead", 200, lead());
    server.respond("POST", "/lead/prompt", 200, lead());
    renderApp({ route: "/chat", initial: server });

    const card = await leadCard();
    await user.click(within(card).getByRole("button", { name: "Yes, proceed" }));
    await waitFor(() => expect(within(card).getByRole("status")).toHaveTextContent("the Lead is still at this question. You can choose again."));
    expect(server.requests.filter((r) => r.method === "GET" && r.path === "/lead").length).toBeGreaterThan(1);
    expect(within(card).getByRole("button", { name: "No, exit" })).toBeEnabled();
  });

  it("AC1: a question whose choices could not be read says so and offers its one answer", async () => {
    const user = userEvent.setup();
    const screenText = { text: "some screen the app could not read", options: [{ id: "answered_in_pane", label: "I answered it in the pane" }] };
    server.respond("GET", "/lead", 200, lead({ prompt: screenText }));
    server.respond("POST", "/lead/prompt", 200, lead({ status: "working", prompt: null }));
    renderApp({ route: "/chat", initial: server });

    const card = await leadCard();
    expect(card).toHaveTextContent("The app could not read the choices of this question");
    await user.click(within(card).getByRole("button", { name: "I answered it in the pane" }));
    await waitFor(() => expect(posts("/lead/prompt")).toHaveLength(1));
    expect(JSON.parse(posts("/lead/prompt")[0]!.body)).toEqual({ option: "answered_in_pane" });
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from the Lead" })).toBeNull());
  });

  it("shows no card when the Lead has no prompt", async () => {
    server.respond("GET", "/lead", 200, lead({ status: "working", prompt: null }));
    renderApp({ route: "/chat", initial: server });
    expect(await screen.findByText("Lead running")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Question from the Lead" })).toBeNull();
  });
});

describe("a blocked worker's prompt in the Attention Inbox", () => {
  const openInbox = (items: AttentionItem[]) => {
    server.respond("GET", "/lead", 200, lead({ status: "working", prompt: null }));
    server.respond("GET", "/attention", 200, { items });
    renderApp({ route: "/chat", initial: server });
  };

  it("AC1/AC2: shows the question with one button per choice; a double click answers once, and the card goes when the stream removes the item", async () => {
    const user = userEvent.setup();
    openInbox([blocked()]);
    server.respond("POST", PROMPT_PATH, 200, { items: [blocked()] });
    const release = server.hold("POST", PROMPT_PATH);

    const card = await workerCard();
    expect(card).toHaveTextContent("Allow this command?");
    expect(card).toHaveTextContent("cargo test");
    expect(within(card).getAllByRole("button").map((b) => b.textContent)).toEqual(["Yes", "No"]);
    // the answer is given here: no pointer to Herdr
    expect(screen.queryByRole("button", { name: /Open Pane in Herdr/ })).toBeNull();

    await user.dblClick(within(card).getByRole("button", { name: "Yes" }));
    expect(await within(card).findByRole("button", { name: "Sending “Yes”…" })).toBeDisabled();
    await waitFor(() => expect(posts(PROMPT_PATH)).toHaveLength(1));
    release();
    await waitFor(() => expect(within(card).getByRole("status")).toHaveTextContent("Waiting for rs-claude-1 to carry on"));
    expect(posts(PROMPT_PATH)).toHaveLength(1);
    expect(JSON.parse(posts(PROMPT_PATH)[0]!.body)).toEqual({ option: "1-aa" });

    await live("attention", { op: "remove", item: blocked({ prompt: null }) });
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from rs-claude-1" })).toBeNull());
    expect(posts(PROMPT_PATH)).toHaveLength(1);
  });

  it("AC2: the item stays open but its prompt goes (no stream event): a fresh read after the answer takes the card away", async () => {
    const user = userEvent.setup();
    openInbox([blocked()]);
    // the answer is typed; the worker has not moved on yet when the server answers
    server.respond("POST", PROMPT_PATH, 200, { items: [blocked()] });

    const card = await workerCard();
    await user.click(within(card).getByRole("button", { name: "Yes" }));
    await waitFor(() => expect(within(card).getByRole("status")).toHaveTextContent("Waiting for rs-claude-1 to carry on"));
    // the worker carries on: its item stays open (the milestone is still running), without a prompt
    server.respond("GET", "/attention", 200, { items: [blocked({ prompt: null })] });
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from rs-claude-1" })).toBeNull());
    expect(screen.getByText("rs-claude-1 is blocked on permission")).toBeInTheDocument();
    expect(posts(PROMPT_PATH)).toHaveLength(1);
  });

  it("AC2: a new question on the same item reaches the card, ready to answer", async () => {
    const user = userEvent.setup();
    openInbox([blocked()]);
    server.respond("POST", PROMPT_PATH, 200, { items: [blocked()] });

    const card = await workerCard();
    await user.click(within(card).getByRole("button", { name: "Yes" }));
    await waitFor(() => expect(within(card).getByRole("status")).toHaveTextContent("Waiting for rs-claude-1 to carry on"));
    const next = { text: "Write to Cargo.lock?", options: [{ id: "1-bb", label: "Allow" }, { id: "2-bb", label: "Deny" }] };
    server.respond("GET", "/attention", 200, { items: [blocked({ prompt: next })] });
    await waitFor(async () => expect(await workerCard()).toHaveTextContent("Write to Cargo.lock?"));
    const again = await workerCard();
    expect(within(again).getByRole("button", { name: "Allow" })).toBeEnabled();
    expect(within(again).getByRole("status")).not.toHaveTextContent("Waiting");
  });

  it("AC2: a new item that a re-read finds first still alerts when its event comes", async () => {
    const user = userEvent.setup();
    openInbox([blocked()]);
    server.respond("POST", PROMPT_PATH, 200, { items: [blocked()] });
    const card = await workerCard();
    await user.click(within(card).getByRole("button", { name: "Yes" }));

    // the re-read after the answer lists a new item before the feed publishes it
    const plan: AttentionItem = {
      id: "plan:1",
      kind: "plan_approval",
      task: null,
      agent: null,
      title: "The plan is ready for approval",
      detail: null,
      prompt: null,
      since: "2026-11-22T10:05:00Z",
      actions: ["approve"],
    };
    server.respond("GET", "/attention", 200, { items: [blocked(), plan] });
    const inbox = await screen.findByRole("list", { name: "Waiting items" });
    await within(inbox).findByText("The plan is ready for approval");
    expect(screen.queryByRole("alertdialog")).toBeNull();

    await live("attention", { op: "add", item: plan });
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("The plan is ready for approval");
  });

  it("AC2: a prompt answered in the pane goes without an answer from the app", async () => {
    attentionTiming.promptPollMs = 50;
    openInbox([blocked()]);
    await workerCard();
    server.respond("GET", "/attention", 200, { items: [blocked({ prompt: null })] });
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from rs-claude-1" })).toBeNull());
    expect(screen.getByText("rs-claude-1 is blocked on permission")).toBeInTheDocument();
  });

  it("AC2: an answer refused with no_prompt reads the items again, and a gone prompt takes the card away", async () => {
    const user = userEvent.setup();
    openInbox([blocked()]);
    server.respond("POST", PROMPT_PATH, 409, errorBody("no_prompt", "rs-claude-1 is not at a prompt"));

    const card = await workerCard();
    server.respond("GET", "/attention", 200, { items: [blocked({ prompt: null })] });
    await user.click(within(card).getByRole("button", { name: "No" }));
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from rs-claude-1" })).toBeNull());
  });

  it("AC3: a refused answer shows the server's words with Try again, and the card stays", async () => {
    const user = userEvent.setup();
    openInbox([blocked()]);
    server.respond(
      "POST",
      PROMPT_PATH,
      400,
      errorBody("bad_option", "`1-aa` is not one of the answers `rs-claude-1`'s prompt offers now, so nothing was typed"),
    );

    const card = await workerCard();
    await user.click(within(card).getByRole("button", { name: "Yes" }));
    const alert = await within(card).findByRole("alert");
    expect(alert).toHaveTextContent("is not one of the answers `rs-claude-1`'s prompt offers now, so nothing was typed");
    expect(within(alert).getByRole("button", { name: "Try again" })).toBeEnabled();
    expect(await workerCard()).toBeInTheDocument();

    server.respond("POST", PROMPT_PATH, 200, { items: [] });
    await user.click(within(alert).getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(posts(PROMPT_PATH)).toHaveLength(2));
    // the answer listed no open items: the item is resolved
    await waitFor(() => expect(screen.queryByRole("group", { name: "Question from rs-claude-1" })).toBeNull());
  });
});

describe("a blocked worker's prompt in the Attention Center", () => {
  it("AC1: shows the question with its choices and answers it once", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/attention", 200, { items: [blocked()] });
    server.respond("POST", PROMPT_PATH, 200, { items: [blocked()] });
    renderApp({ route: "/notifications", initial: server });

    const card = await screen.findByRole("group", { name: "Question from rs-claude-1" });
    expect(card).toHaveTextContent("Allow this command?");
    await user.dblClick(within(card).getByRole("button", { name: "No" }));
    await waitFor(() => expect(posts(PROMPT_PATH)).toHaveLength(1));
    expect(JSON.parse(posts(PROMPT_PATH)[0]!.body)).toEqual({ option: "2-aa" });
    expect(screen.queryByRole("button", { name: /Open Pane in Herdr/ })).toBeNull();
  });
});

describe("a blocked worker's prompt in the attention popup", () => {
  it("AC2: answers once; a re-read's new question replaces the old one, and a gone question leaves the popup", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/lead", 200, lead({ status: "working", prompt: null }));
    server.respond("GET", "/attention", 200, { items: [] });
    server.respond("POST", PROMPT_PATH, 200, { items: [blocked()] });
    renderApp({ route: "/chat", initial: server });
    // the first read finds nothing waiting; the worker blocks afterwards
    await waitFor(() => expect(server.requests.some((r) => r.method === "GET" && r.path === "/attention")).toBe(true));
    await screen.findByText("Lead running");

    server.respond("GET", "/attention", 200, { items: [blocked()] });
    await live("attention", { op: "add", item: blocked() });
    const dialog = await screen.findByRole("alertdialog");
    const card = within(dialog).getByRole("group", { name: "Question from rs-claude-1" });
    expect(card).toHaveTextContent("Allow this command?");
    await user.dblClick(within(card).getByRole("button", { name: "Yes" }));
    await waitFor(() => expect(within(card).getByRole("status")).toHaveTextContent("Waiting for rs-claude-1 to carry on"));
    expect(posts(PROMPT_PATH)).toHaveLength(1);

    // the worker carried on to a new question: the popup shows it, ready to answer
    const next = { text: "Write to Cargo.lock?", options: [{ id: "1-bb", label: "Allow" }, { id: "2-bb", label: "Deny" }] };
    server.respond("GET", "/attention", 200, { items: [blocked({ prompt: next })] });
    await waitFor(() => expect(within(dialog).getByRole("group", { name: "Question from rs-claude-1" })).toHaveTextContent("Write to Cargo.lock?"));
    expect(within(dialog).queryByText(/Allow this command/)).toBeNull();
    expect(within(dialog).getByRole("button", { name: "Allow" })).toBeEnabled();

    // answered, and the worker carries on without a question: the popup no longer shows one
    server.respond("POST", PROMPT_PATH, 200, { items: [blocked({ prompt: next })] });
    await user.click(within(dialog).getByRole("button", { name: "Allow" }));
    server.respond("GET", "/attention", 200, { items: [blocked({ prompt: null })] });
    await waitFor(() => expect(within(dialog).queryByRole("group", { name: "Question from rs-claude-1" })).toBeNull());
    expect(posts(PROMPT_PATH)).toHaveLength(2);
  });
});
