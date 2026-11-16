import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createApiClient, normalizeUrl } from "./client";
import { samples } from "./contract";
import { ApiError } from "./errors";
import { deadAddress, errorBody, startFakeServer, type FakeServer } from "../test/fakeServer";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
});
afterEach(() => server.close());

const client = () => createApiClient({ url: server.url, token: server.token });

describe("api client", () => {
  it("returns the typed body and sends the bearer token", async () => {
    const health = await client().health();
    expect(health).toEqual({ ...samples.health, run: true });
    expect(server.requests[0]?.headers.authorization).toBe(`Bearer ${server.token}`);
  });

  it("covers every read route with its query", async () => {
    const c = client();
    const reads: [string, () => Promise<unknown>, unknown][] = [
      ["/status", () => c.status(), samples.status],
      ["/plan", () => c.plan(), samples.plan],
      ["/tasks", () => c.tasks(), samples.tasks],
      ["/tasks/M1", () => c.task("M1"), samples.taskDetail],
      ["/events", () => c.events({ since: 3, limit: 10 }), samples.events],
      ["/report", () => c.report(), samples.report],
      ["/agents", () => c.agents(), samples.agents],
      ["/agents/lead/output", () => c.agentOutput("lead", { lines: 100 }), samples.agentOutput],
      ["/models", () => c.models(), samples.models],
      ["/logs", () => c.logs({ lines: 50, milestone: "M1" }), samples.logs],
      ["/attention", () => c.attention(), samples.attention],
      ["/openapi.json", () => c.openapi(), { openapi: "3.1.0", info: { title: "t", version: "1" }, paths: {} }],
    ];
    for (const [path, , body] of reads) server.respond("GET", path, 200, body);
    for (const [path, call, body] of reads) {
      expect(await call(), path).toEqual(body);
    }
    const events = server.requests.find((r) => r.path === "/events");
    expect(events?.query.get("since")).toBe("3");
    expect(events?.query.get("limit")).toBe("10");
    const logs = server.requests.find((r) => r.path === "/logs");
    expect([logs?.query.get("lines"), logs?.query.get("milestone")]).toEqual(["50", "M1"]);
  });

  it("covers every action route: method, path and JSON body", async () => {
    const c = client();
    for (const p of ["/dispatch", "/tasks/M1/answer", "/tasks/M1/approve", "/tasks/M1/escalate", "/tasks/M1/fix", "/tasks/M1/status"]) {
      server.respond("POST", p, 200, samples.action);
    }
    for (const p of ["/run/pause", "/run/resume", "/run/stop"]) server.respond("POST", p, 200, samples.runControl);

    expect(await c.dispatch({ ids: ["M1"], tier: "high" })).toEqual(samples.action);
    await c.answer("M1", { text: "yes" });
    await c.approve("M1");
    await c.escalate("M1", { reason: "stuck" });
    await c.fix("M1", { text: "rename it" });
    await c.setStatus("M1", { status: "paused" });
    expect((await c.pauseRun()).action).toBe(samples.runControl.action);
    await c.resumeRun();
    await c.stopRun();

    const posts = server.requests.filter((r) => r.method === "POST");
    expect(posts.map((r) => r.path)).toEqual([
      "/dispatch",
      "/tasks/M1/answer",
      "/tasks/M1/approve",
      "/tasks/M1/escalate",
      "/tasks/M1/fix",
      "/tasks/M1/status",
      "/run/pause",
      "/run/resume",
      "/run/stop",
    ]);
    expect(JSON.parse(posts[0]?.body ?? "")).toEqual({ ids: ["M1"], tier: "high" });
    expect(posts[0]?.headers["content-type"]).toBe("application/json");
    expect(JSON.parse(posts[1]?.body ?? "")).toEqual({ text: "yes" });
  });

  it("encodes the milestone id in the path", async () => {
    server.respond("GET", "/tasks/a%2Fb", 404, errorBody("not_found", "no milestone"));
    await expect(client().task("a/b")).rejects.toMatchObject({ code: "not_found" });
  });

  it("turns a 401 into the typed error", async () => {
    server.rejectToken();
    const error = await client().status().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ code: "unauthorized", status: 401, requestId: "req-test", isUnauthorized: true });
  });

  it("turns a 404 into the typed error (no run, or no such milestone)", async () => {
    server.respond("GET", "/status", 404, errorBody("no_run", "no run exists in this project"));
    const noRun = await client().status().catch((e: unknown) => e);
    expect(noRun).toMatchObject({ code: "no_run", status: 404, isNoRun: true });
    const missing = await client().task("M9").catch((e: unknown) => e);
    expect(missing).toMatchObject({ code: "not_found", status: 404 });
  });

  it("turns a 409 into the typed error with the server's reason", async () => {
    server.respond("POST", "/tasks/M1/approve", 409, errorBody("conflict", "M1 is merged: nothing to approve"));
    const error = await client().approve("M1").catch((e: unknown) => e);
    expect(error).toMatchObject({ code: "conflict", status: 409, message: "M1 is merged: nothing to approve" });
  });

  it("reports an unreachable server as a network error without the token in it", async () => {
    const dead = createApiClient({ url: await deadAddress(), token: "secret-token-value" });
    const error = await dead.health().catch((e: unknown) => e);
    expect(error).toMatchObject({ code: "network", status: 0, isNetwork: true });
    expect(String((error as Error).message)).not.toContain("secret-token-value");
  });

  it("reports an answer that is not the API's as bad_response", async () => {
    const fake = createApiClient({
      url: "http://x.test",
      token: "t",
      fetch: async () => new Response("<html>oops</html>", { status: 502 }),
    });
    await expect(fake.health()).rejects.toMatchObject({ code: "bad_response", status: 502 });
  });

  it("normalises the address the user typed", () => {
    expect(normalizeUrl(" 127.0.0.1:8787/ ")).toBe("http://127.0.0.1:8787");
    expect(normalizeUrl("http://localhost:1///")).toBe("http://localhost:1");
  });
});
