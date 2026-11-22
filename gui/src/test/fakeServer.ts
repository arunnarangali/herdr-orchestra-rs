import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { samples } from "../api/contract";

export interface SeenRequest {
  method: string;
  path: string;
  query: URLSearchParams;
  headers: IncomingMessage["headers"];
  body: string;
}

export interface FakeServer {
  url: string;
  token: string;
  /** Every request received, in order. */
  requests: SeenRequest[];
  /** Answer `METHOD path` (without the `/api/v1` prefix) with this status and JSON body. */
  respond: (method: string, path: string, status: number, body: unknown) => void;
  /**
   * Keep requests to `METHOD path` waiting (no answer yet) until the returned `release` is
   * called; then they get the route's answer as it is at that moment. For tests that look at
   * a request while it is in flight, with no timing to race.
   */
  hold: (method: string, path: string) => () => void;
  /** Answer 401 to everything, as if the token had been replaced. */
  rejectToken: (reject?: boolean) => void;
  /** Send an event on every open stream. */
  emit: (id: number, event: string, data: unknown) => void;
  /** Cut every open stream (a network drop). */
  dropStreams: () => void;
  /** Port it listens on (to start a replacement on the same address). */
  port: number;
  /** Stream requests received so far. */
  streamRequests: () => SeenRequest[];
  close: () => Promise<void>;
}

export function errorBody(code: string, message: string) {
  return { error: { code, message, request_id: "req-test" } };
}

/**
 * A stand-in for `orch serve` on a real loopback socket. It checks the bearer token (or the
 * `token` query on the stream) like the real one, answers `/health` and the stream by
 * default, and answers any other route only when the test registered it.
 */
export async function startFakeServer(options: { token?: string; run?: boolean; port?: number } = {}): Promise<FakeServer> {
  const token = options.token ?? "t".repeat(64);
  const requests: SeenRequest[] = [];
  const routes = new Map<string, { status: number; body: unknown }>();
  /** Held routes and the requests waiting on each. */
  const held = new Map<string, (() => void)[]>();
  const streams = new Set<ServerResponse>();
  let rejectAll = false;

  const server: Server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://fake");
    const chunks: Buffer[] = [];
    req.on("data", (c: Buffer) => chunks.push(c));
    req.on("end", () => {
      const seen: SeenRequest = {
        method: req.method ?? "GET",
        path: url.pathname.replace(/^\/api\/v1/, ""),
        query: url.searchParams,
        headers: req.headers,
        body: Buffer.concat(chunks).toString(),
      };
      requests.push(seen);
      const send = (status: number, body: unknown) => {
        res.writeHead(status, { "content-type": "application/json" });
        res.end(JSON.stringify(body));
      };
      const presented =
        req.headers.authorization?.replace(/^Bearer /i, "") ??
        (seen.path === "/stream" ? url.searchParams.get("token") : null);
      if (rejectAll || presented !== token) {
        return send(401, errorBody("unauthorized", "missing or wrong API token"));
      }
      if (seen.path === "/stream") {
        res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
        res.write(": keep-alive\n\n");
        streams.add(res);
        res.on("close", () => streams.delete(res));
        return;
      }
      const key = `${seen.method} ${seen.path}`;
      const answer = () => {
        if (res.destroyed) return;
        const custom = routes.get(key);
        if (custom) return send(custom.status, custom.body);
        if (seen.method === "GET" && seen.path === "/health") {
          return send(200, { ...samples.health, run: options.run ?? true });
        }
        send(404, errorBody("not_found", "no such route"));
      };
      const waiting = held.get(key);
      if (waiting) return void waiting.push(answer);
      answer();
    });
  });
  await new Promise<void>((resolve) => server.listen(options.port ?? 0, "127.0.0.1", resolve));
  const { port } = server.address() as AddressInfo;

  return {
    url: `http://127.0.0.1:${port}`,
    port,
    token,
    requests,
    respond: (method, path, status, body) => void routes.set(`${method} ${path}`, { status, body }),
    hold: (method, path) => {
      const key = `${method} ${path}`;
      if (!held.has(key)) held.set(key, []);
      return () => {
        const waiting = held.get(key) ?? [];
        held.delete(key);
        for (const answer of waiting) answer();
      };
    },
    rejectToken: (reject = true) => void (rejectAll = reject),
    emit: (id, event, data) => {
      for (const s of streams) s.write(`id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
    },
    dropStreams: () => {
      for (const s of streams) s.destroy();
      streams.clear();
    },
    streamRequests: () => requests.filter((r) => r.path === "/stream"),
    close: () =>
      new Promise<void>((resolve) => {
        if (!server.listening) return resolve();
        for (const s of streams) s.destroy();
        server.close(() => resolve());
        server.closeAllConnections();
      }),
  };
}

/** An address nothing listens on (a server that is down). */
export async function deadAddress(): Promise<string> {
  const s = await startFakeServer();
  const url = s.url;
  await s.close();
  return url;
}
