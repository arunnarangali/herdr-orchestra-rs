// Compile-time half of the API contract. `contract/*.json` is produced by the Rust test
// `crates/orch-api/tests/contract.rs` from the server's real shapes. Each sample must fit its
// TypeScript type *exactly*: a missing field, a wrong type, or a field the type does not
// know about (the server grew one) is a type error, so `npm run typecheck` fails until
// `types.ts` is updated. Request samples are checked the same way; the Rust test reads them
// into the real request types.
import action from "./contract/ActionResponse.json";
import agentOutput from "./contract/AgentOutputResponse.json";
import agents from "./contract/AgentsResponse.json";
import answerBody from "./contract/AnswerBody.json";
import attention from "./contract/AttentionResponse.json";
import attentionEvent from "./contract/AttentionEvent.json";
import chatBody from "./contract/ChatBody.json";
import chatList from "./contract/ChatListResponse.json";
import chatMessage from "./contract/ChatMessageView.json";
import dispatchBody from "./contract/DispatchBody.json";
import errorBody from "./contract/ErrorBody.json";
import escalateBody from "./contract/EscalateBody.json";
import events from "./contract/EventsResponse.json";
import fixBody from "./contract/FixBody.json";
import health from "./contract/Health.json";
import leadEvent from "./contract/LeadQueueEvent.json";
import leadStartBody from "./contract/LeadStartBody.json";
import leadView from "./contract/LeadView.json";
import logEvent from "./contract/LogEvent.json";
import logs from "./contract/LogsResponse.json";
import models from "./contract/ModelsResponse.json";
import plan from "./contract/PlanResponse.json";
import report from "./contract/ReportResponse.json";
import runControl from "./contract/RunControlResponse.json";
import runStateEvent from "./contract/RunStateEvent.json";
import setStatusBody from "./contract/SetStatusBody.json";
import status from "./contract/StatusResponse.json";
import streamMessage from "./contract/StreamMessage.json";
import taskDetail from "./contract/TaskDetailResponse.json";
import taskStateEvent from "./contract/TaskStateEvent.json";
import tasks from "./contract/TasksResponse.json";
import type * as T from "./types";

/** `S` with every property `T` does not have turned into `never`. */
type Exact<Want, S> = unknown extends Want
  ? S
  : S extends readonly (infer E)[]
  ? NonNullable<Want> extends readonly (infer W)[]
    ? readonly Exact<W, E>[]
    : never
  : S extends object
    ? { [K in keyof S]: K extends keyof NonNullable<Want> ? Exact<NonNullable<Want>[K], S[K]> : never }
    : S;

/** Check that sample `S` is a `Want` and has no property `Want` lacks. */
const fits =
  <Want>() =>
  <S extends Want>(sample: S & Exact<Want, S>): S =>
    sample;

export const samples = {
  health: fits<T.Health>()(health),
  errorBody: fits<T.ErrorBody>()(errorBody),
  status: fits<T.StatusResponse>()(status),
  plan: fits<T.PlanResponse>()(plan),
  tasks: fits<T.TasksResponse>()(tasks),
  taskDetail: fits<T.TaskDetailResponse>()(taskDetail),
  events: fits<T.EventsResponse>()(events),
  report: fits<T.ReportResponse>()(report),
  agents: fits<T.AgentsResponse>()(agents),
  models: fits<T.ModelsResponse>()(models),
  logs: fits<T.LogsResponse>()(logs),
  attention: fits<T.AttentionResponse>()(attention),
  agentOutput: fits<T.AgentOutputResponse>()(agentOutput),
  action: fits<T.ActionResponse>()(action),
  runControl: fits<T.RunControlResponse>()(runControl),
  chatMessage: fits<T.ChatMessage>()(chatMessage),
  chatList: fits<T.ChatListResponse>()(chatList),
  streamMessage: fits<T.StreamMessageEnvelope>()(streamMessage),
  taskStateEvent: fits<T.TaskStateEvent>()(taskStateEvent),
  leadEvent: fits<T.LeadQueueEvent>()(leadEvent),
  runStateEvent: fits<T.RunStateEvent>()(runStateEvent),
  logEvent: fits<T.LogEvent>()(logEvent),
  attentionEvent: fits<T.AttentionEvent>()(attentionEvent),
  leadView: fits<T.LeadView>()(leadView),
  leadStartBody: fits<T.LeadStartBody>()(leadStartBody),
  answerBody: fits<T.AnswerBody>()(answerBody),
  chatBody: fits<T.ChatBody>()(chatBody),
  dispatchBody: fits<T.DispatchBody>()(dispatchBody),
  fixBody: fits<T.FixBody>()(fixBody),
  escalateBody: fits<T.EscalateBody>()(escalateBody),
  setStatusBody: fits<T.SetStatusBody>()(setStatusBody),
};
