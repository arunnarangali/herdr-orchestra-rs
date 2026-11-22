import { useCallback, useState } from "react";
import { isApiError } from "../api/errors";
import type { ChatBody, ChatMessage } from "../api/types";
import { useApi } from "../app/connection";

/** The API's code when no lead pane can take a message (the message is stored all the same). */
export const LEAD_UNAVAILABLE = "lead_unavailable";

export type SendResult =
  | { ok: true; message: ChatMessage }
  /** The message is stored; no lead can take it yet. `text` says how to start one. */
  | { ok: false; kind: "lead_unavailable"; text: string }
  | { ok: false; kind: "error"; text: string };

/**
 * Send one message to the lead through `POST /chat`. The GUI only asks: the server stores the
 * message and the event manager delivers it. Never throws; the result says what happened.
 */
export function useSendChat() {
  const api = useApi();
  const [pending, setPending] = useState(false);
  const send = useCallback(
    async (body: ChatBody): Promise<SendResult> => {
      setPending(true);
      try {
        return { ok: true, message: await api.sendChat(body) };
      } catch (e) {
        if (isApiError(e) && e.code === LEAD_UNAVAILABLE) return { ok: false, kind: "lead_unavailable", text: e.message };
        return { ok: false, kind: "error", text: isApiError(e) ? e.message : String(e) };
      } finally {
        setPending(false);
      }
    },
    [api],
  );
  return { send, pending };
}

/** The notice shown when the lead cannot take a message: what it means, and how to start one. */
export function LeadUnavailable({ text }: { text: string }) {
  return (
    <div className="notice notice--warning lead-unavailable" role="alert">
      <div>
        <p className="panel-title">The lead is not running</p>
        <p>{text}</p>
        <p className="muted">Press <strong>Start Lead</strong> in Chat to start it.</p>
      </div>
    </div>
  );
}
