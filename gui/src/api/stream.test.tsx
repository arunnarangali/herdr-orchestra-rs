import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MESSAGES_KEPT, openEventStream, useEventStream, type StreamStatus } from "./stream";
import type { StreamMessage } from "./types";
import { deadAddress, startFakeServer, type FakeServer } from "../test/fakeServer";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
});
afterEach(() => server.close());

function open(creds: { url: string; token: string }) {
  const statuses: StreamStatus[] = [];
  const refusals: (number | null)[] = [];
  const messages: StreamMessage[] = [];
  const close = openEventStream({
    ...creds,
    retryDelaysMs: [15],
    onStatus: (s, r) => {
      statuses.push(s);
      refusals.push(r?.status ?? null);
    },
    onMessage: (m) => messages.push(m),
  });
  return { statuses, refusals, messages, close };
}

describe("openEventStream", () => {
  it("connects with the token as the `token` query parameter and no header", async () => {
    const s = open(server);
    await waitFor(() => expect(s.statuses).toContain("connected"));
    const req = server.streamRequests()[0];
    expect(req?.query.get("token")).toBe(server.token);
    expect(req?.headers.authorization).toBeUndefined();
    expect(req?.headers["last-event-id"]).toBeUndefined();
    s.close();
  });

  it("delivers messages typed by event name and ignores unknown events", async () => {
    const s = open(server);
    await waitFor(() => expect(s.statuses).toContain("connected"));
    server.emit(1, "task-state", { id: "M1", to: "coding" });
    server.emit(2, "something-else", { x: 1 });
    server.emit(3, "run-state", { run_control: "paused" });
    await waitFor(() => expect(s.messages).toHaveLength(2));
    expect(s.messages.map((m) => [m.id, m.event])).toEqual([
      ["1", "task-state"],
      ["3", "run-state"],
    ]);
    s.close();
  });

  it("reconnects after a drop with Last-Event-ID and carries on", async () => {
    const s = open(server);
    await waitFor(() => expect(s.statuses).toContain("connected"));
    server.emit(41, "log", { message: "a", line: "a", ts: 1 });
    server.emit(42, "log", { message: "b", line: "b", ts: 2 });
    await waitFor(() => expect(s.messages).toHaveLength(2));

    server.dropStreams();
    await waitFor(() => expect(s.statuses).toContain("reconnecting"));
    await waitFor(() => expect(server.streamRequests()).toHaveLength(2));
    expect(server.streamRequests()[1]?.headers["last-event-id"]).toBe("42");
    await waitFor(() => expect(s.statuses.at(-1)).toBe("connected"));

    server.emit(43, "log", { message: "c", line: "c", ts: 3 });
    await waitFor(() => expect(s.messages).toHaveLength(3));
    expect(s.statuses).toEqual(["connecting", "connected", "reconnecting", "connected"]);
    s.close();
  });

  it("reports refused for a wrong token and does not retry", async () => {
    const s = open({ url: server.url, token: "wrong" });
    await waitFor(() => expect(s.statuses.at(-1)).toBe("refused"));
    expect(s.refusals.at(-1)).toBe(401);
    await new Promise((r) => setTimeout(r, 100));
    expect(server.streamRequests()).toHaveLength(1);
    expect(s.statuses).toEqual(["connecting", "refused"]);
    s.close();
  });

  it("keeps reconnecting while the server is down, then connects when it is back", async () => {
    const url = await deadAddress();
    const port = Number(new URL(url).port);
    const s = open({ url, token: server.token });
    await waitFor(() => expect(s.statuses).toContain("reconnecting"));
    expect(s.statuses).not.toContain("connected");

    const back = await startFakeServer({ port, token: server.token });
    await waitFor(() => expect(s.statuses.at(-1)).toBe("connected"));
    s.close();
    await back.close();
  });

  it("stops for good once closed", async () => {
    const s = open(server);
    await waitFor(() => expect(s.statuses).toContain("connected"));
    s.close();
    server.dropStreams();
    await new Promise((r) => setTimeout(r, 80));
    expect(server.streamRequests()).toHaveLength(1);
  });
});

describe("useEventStream", () => {
  it("exposes status and the messages by event name, and tells subscribers", async () => {
    const { result } = renderHook(() => useEventStream(server, { retryDelaysMs: [15] }));
    expect(["idle", "connecting"]).toContain(result.current.status);
    const heard: string[] = [];
    result.current.subscribe((m) => heard.push(m.event));
    await waitFor(() => expect(result.current.status).toBe("connected"));

    server.emit(1, "lead-event", { id: 1, ts: 1, kind: "needs-answer", text: "q" });
    server.emit(2, "task-state", { id: "M1" });
    await waitFor(() => expect(result.current.messages["task-state"]).toHaveLength(1));
    expect(result.current.messages["lead-event"]).toHaveLength(1);
    expect(result.current.messages.log).toHaveLength(0);
    expect(heard).toEqual(["lead-event", "task-state"]);
  });

  it("is idle without credentials and refused with a wrong token", async () => {
    const none = renderHook(() => useEventStream(null));
    expect(none.result.current.status).toBe("idle");
    const bad = renderHook(() => useEventStream({ url: server.url, token: "nope" }, { retryDelaysMs: [15] }));
    await waitFor(() => expect(bad.result.current.status).toBe("refused"));
    expect(bad.result.current.refusal).toEqual({ status: 401 });
  });

  it("keeps only the newest messages", async () => {
    const { result } = renderHook(() => useEventStream(server, { retryDelaysMs: [15] }));
    await waitFor(() => expect(result.current.status).toBe("connected"));
    for (let i = 1; i <= MESSAGES_KEPT + 5; i++) server.emit(i, "log", { message: String(i), line: String(i), ts: i });
    await waitFor(() => expect(result.current.messages.log.at(-1)?.id).toBe(String(MESSAGES_KEPT + 5)));
    expect(result.current.messages.log).toHaveLength(MESSAGES_KEPT);
    expect(result.current.messages.log[0]?.id).toBe("6");
  });
});
