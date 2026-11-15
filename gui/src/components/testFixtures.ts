// Test data for the milestone screens, built from the contract samples the Rust tests produce.
import { samples } from "../api/contract";
import type { EventsResponse, StatusResponse, TaskDetailResponse, TaskInfo } from "../api/types";
import type { FakeServer } from "../test/fakeServer";

export const task = (o: Partial<TaskInfo> = {}): TaskInfo => Object.assign<TaskInfo, Partial<TaskInfo>>({ ...samples.tasks.tasks[0]! }, o);

export const status = (o: Partial<StatusResponse> = {}): StatusResponse => ({
  ...samples.status,
  broken_branch: null,
  run_state: "running",
  stuck: false,
  progress: [
    { state: "merged", count: 1 },
    { state: "coding", count: 1 },
    { state: "pending", count: 1 },
  ],
  workers: [
    { name: "rs-claude-1", kind: "claude", model: "claude-sonnet-5-5", tier: "medium", detect: true, task: "M2" },
    { name: "rs-codex-1", kind: "codex", model: null, tier: null, detect: true, task: null },
  ],
  tasks: [
    { id: "M1", title: "Foundation", state: "merged", deps: [], worker: null, tier: "medium", reason: "" },
    { id: "M2", title: "Overview", state: "coding", deps: ["M1"], worker: "rs-claude-1", tier: "medium", reason: "" },
    { id: "M3", title: "Reports", state: "pending", deps: ["M2"], worker: null, tier: "small", reason: "" },
  ],
  ...o,
});

export const events = (o: Partial<EventsResponse> = {}): EventsResponse => ({ ...samples.events, ...o });

export const detail = (t: TaskInfo, o: Partial<TaskDetailResponse> = {}): TaskDetailResponse => ({
  ...samples.taskDetail,
  task: t,
  ...o,
});

/** Make the fake server answer the routes both screens read. */
export function serve(server: FakeServer, o: { status?: StatusResponse; tasks?: TaskInfo[]; events?: EventsResponse; detail?: TaskDetailResponse; logs?: string[] } = {}) {
  const tasks = o.tasks ?? [task()];
  server.respond("GET", "/status", 200, o.status ?? status());
  server.respond("GET", "/tasks", 200, { tasks });
  server.respond("GET", "/events", 200, o.events ?? events());
  server.respond("GET", "/logs", 200, { lines: o.logs ?? ["12:00 dispatched M1"] });
  for (const t of tasks) server.respond("GET", `/tasks/${t.id}`, 200, o.detail ?? detail(t));
}
