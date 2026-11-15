import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { samples } from "../../api/contract";
import type { ChatMessage } from "../../api/types";
import { errorBody, startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";
import { parallelPlan } from "./fixtures";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
});
afterEach(() => server.close());

const approval = (over: Partial<ChatMessage> = {}): ChatMessage => ({
  id: 7,
  ts: "2026-11-15T09:30:00Z",
  sender: "developer",
  kind: "plan_approval",
  text: "Go: the plan is approved. Start the run.",
  request: null,
  about: null,
  delivery: "queued",
  delivery_note: null,
  ...over,
});
const tasksIn = (state: string) => ({
  tasks: ["M1", "M2", "M3"].map((id) => ({ ...samples.tasks.tasks[0]!, id, state })),
});
const show = (state = "pending", messages: ChatMessage[] = []) => {
  server.respond("GET", "/plan", 200, parallelPlan());
  server.respond("GET", "/tasks", 200, tasksIn(state));
  server.respond("GET", "/chat", 200, { messages });
  return renderApp({ route: "/workflows", initial: server });
};
const posts = () => server.requests.filter((r) => r.method === "POST" && r.path === "/chat");

describe("plan approval on the Workflows screen", () => {
  it("shows the waiting plan for review and an Approve and start button", async () => {
    show();
    expect(await screen.findByRole("heading", { name: "Review the plan, then approve it" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve and start" })).toBeEnabled();
    // the plan itself is there to read: milestones, dependencies
    expect(screen.getByRole("heading", { name: "Milestones" })).toBeInTheDocument();
    expect(screen.getAllByText(/after M1/).length).toBeGreaterThan(0);
  });

  it("sends a plan_approval and then says the lead has been told", async () => {
    const user = userEvent.setup();
    show();
    server.respond("POST", "/chat", 201, approval());
    await user.click(await screen.findByRole("button", { name: "Approve and start" }));
    expect(await screen.findByText(/The lead has been told to start the run/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Approve and start" })).not.toBeInTheDocument();
    expect(JSON.parse(posts()[0]!.body)).toEqual({ kind: "plan_approval", text: "", request: null });
  });

  it("remembers an approval already sent, and follows its delivery state live", async () => {
    show("pending", [approval()]);
    expect(await screen.findByText(/The lead has been told/)).toHaveTextContent("waiting for the lead's pane");
    await waitFor(() => expect(server.streamRequests().length).toBeGreaterThan(0));
    // the stream says it changed; the re-read (the real server's answer) agrees
    server.respond("GET", "/chat", 200, { messages: [approval({ delivery: "sent" })] });
    await act(async () => server.emit(1, "chat", approval({ delivery: "sent" })));
    await waitFor(() => expect(screen.getByText(/The lead has been told/)).toHaveTextContent("delivered to the lead"));
  });

  it("offers to send it again when delivery failed", async () => {
    show("pending", [approval({ delivery: "failed", delivery_note: "could not be typed into the lead's pane" })]);
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be typed into the lead's pane");
    expect(screen.getByRole("button", { name: "Approve and start again" })).toBeInTheDocument();
  });

  it("with no lead it says how to start one and keeps the button", async () => {
    const user = userEvent.setup();
    show();
    server.respond("POST", "/chat", 409, errorBody("lead_unavailable", "no lead pane exists: this run has no lead."));
    await user.click(await screen.findByRole("button", { name: "Approve and start" }));
    const alert = (await screen.findByText("The lead is not running")).closest("[role=alert]")!;
    expect(alert).toHaveTextContent("orch lead run");
    expect(screen.getByRole("button", { name: "Approve and start" })).toBeEnabled();
  });

  it("holds the approval back while a milestone has open questions", async () => {
    server.respond("GET", "/plan", 200, parallelPlan());
    server.respond("GET", "/tasks", 200, {
      tasks: [
        { ...samples.tasks.tasks[0]!, id: "M1", state: "pending" },
        { ...samples.tasks.tasks[0]!, id: "M2", state: "needs-answer" },
      ],
    });
    server.respond("GET", "/chat", 200, { messages: [] });
    renderApp({ route: "/workflows", initial: server });
    expect(await screen.findByText(/M2 still has open questions/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve and start" })).toBeDisabled();
  });

  it("is not shown once the run is under way", async () => {
    show("coding");
    await screen.findByRole("heading", { name: "Milestones" });
    expect(screen.queryByRole("button", { name: /Approve and start/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Review the plan, then approve it" })).not.toBeInTheDocument();
  });
});
