import { describe, expect, it } from "vitest";
import { SseParser } from "./sse";

describe("SseParser", () => {
  it("reads id, event and data, and skips comments", () => {
    const p = new SseParser();
    const out = p.push(": keep-alive\n\nid: 4\nevent: log\ndata: {\"a\":1}\n\n");
    expect(out).toEqual([{ id: "4", event: "log", data: '{"a":1}' }]);
  });

  it("handles events split across chunks and CRLF line ends", () => {
    const p = new SseParser();
    expect(p.push("id: 1\r\nevent: run-st")).toEqual([]);
    expect(p.push("ate\r")).toEqual([]);
    expect(p.push("\ndata: {}\r\n\r\n")).toEqual([{ id: "1", event: "run-state", data: "{}" }]);
  });

  it("joins data lines and defaults the event name", () => {
    const out = new SseParser().push("data: a\ndata: b\n\n");
    expect(out).toEqual([{ id: null, event: "message", data: "a\nb" }]);
  });

  it("keeps the last id for events that carry none", () => {
    const p = new SseParser();
    const out = p.push("id: 7\ndata: x\n\ndata: y\n\n");
    expect(out.map((e) => e.id)).toEqual(["7", "7"]);
  });
});
