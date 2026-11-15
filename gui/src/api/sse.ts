/** A parsed Server-Sent Event (only the fields the app uses). */
export interface SseEvent {
  id: string | null;
  event: string;
  data: string;
}

/**
 * Incremental parser for the `text/event-stream` format: feed it chunks of text, get back the
 * events completed by each chunk. Comment lines (`: keep-alive`) are dropped.
 */
export class SseParser {
  private buffer = "";
  private id: string | null = null;
  private event = "";
  private data: string[] = [];
  private lastWasCr = false;

  push(chunk: string): SseEvent[] {
    // A "\r\n" split across two chunks must not count as two line ends.
    if (this.lastWasCr && chunk.startsWith("\n")) chunk = chunk.slice(1);
    this.lastWasCr = chunk.endsWith("\r");
    this.buffer += chunk;
    const events: SseEvent[] = [];
    let match: RegExpExecArray | null;
    const lineEnd = /\r\n|\n|\r/g;
    let start = 0;
    while ((match = lineEnd.exec(this.buffer)) !== null) {
      // A trailing "\r" may be the first half of "\r\n": wait for more input.
      if (match[0] === "\r" && lineEnd.lastIndex === this.buffer.length) break;
      const ev = this.line(this.buffer.slice(start, match.index));
      if (ev) events.push(ev);
      start = lineEnd.lastIndex;
    }
    this.buffer = this.buffer.slice(start);
    return events;
  }

  private line(line: string): SseEvent | null {
    if (line === "") {
      if (this.data.length === 0) {
        this.event = "";
        return null;
      }
      const ev = { id: this.id, event: this.event || "message", data: this.data.join("\n") };
      this.event = "";
      this.data = [];
      return ev;
    }
    if (line.startsWith(":")) return null;
    const colon = line.indexOf(":");
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "id" && !value.includes("\0")) this.id = value;
    else if (field === "event") this.event = value;
    else if (field === "data") this.data.push(value);
    return null;
  }
}
