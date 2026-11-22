import { useEffect, useRef, useState } from "react";
import { isApiError } from "../api/errors";
import { ANSWERED_IN_PANE, type AgentPrompt } from "../api/types";
import { Button, type Tone } from "../ui/Button";
import "./components.css";

/**
 * How an answered card follows the agent. The stream does not say when a worker's prompt goes
 * (its item stays open while the milestone carries on), so after an answer the card asks again
 * every `recheckMs`; once `stillWaitingMs` have passed and a check still shows the same
 * question, the choices can be pressed again. Tests shorten these.
 */
export const promptTiming = { recheckMs: 1_500, stillWaitingMs: 10_000 };

/** Same question, same choices: the identity of a prompt (a new one resets the card). */
export function promptKey(prompt: AgentPrompt): string {
  return [prompt.text, ...prompt.options.map((o) => o.id)].join("\u0000");
}

export interface PromptAnswerProps {
  prompt: AgentPrompt;
  /** Who is asking, in words: "The Lead", "rs-claude-1". */
  who: string;
  /** Send the chosen option's id; rejects with the server's error when it was not typed. */
  onAnswer: (option: string) => Promise<unknown>;
  /**
   * Read the agent's state again and show it (the caller stops rendering the card when the
   * prompt is gone, or passes the new prompt). Rejects when it could not be read.
   */
  onRecheck: () => Promise<unknown>;
  tone?: Tone;
}

type Phase =
  | { kind: "idle" }
  | { kind: "sending"; option: string }
  | { kind: "sent"; option: string; at: number; again: "no" | "checked" | "unchecked"; why?: string }
  | { kind: "error"; option: string; message: string };

/**
 * An agent's open prompt: its text and one button per choice. A choice is sent once (a second
 * press while it is on its way does nothing); the card stays until the agent moves on, which the
 * caller learns from the live stream or from `onRecheck`, and shows by no longer rendering it.
 */
export function PromptAnswer({ prompt, who, onAnswer, onRecheck, tone = "amber" }: PromptAnswerProps) {
  const key = promptKey(prompt);
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const busy = useRef(false);

  // a different question (or other choices) is a new prompt: forget the last answer
  const [shownKey, setShownKey] = useState(key);
  if (shownKey !== key) {
    setShownKey(key);
    setPhase({ kind: "idle" });
  }

  const recheck = useRef(onRecheck);
  recheck.current = onRecheck;

  // answered: ask again until the agent has moved on (the card then goes, or shows its next
  // question); after a while, a check that still shows this question lets the developer choose again
  useEffect(() => {
    if (phase.kind !== "sent" || phase.again !== "no") return;
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const tick = () => {
      recheck.current().then(
        () => {
          if (!live) return;
          if (Date.now() - phase.at >= promptTiming.stillWaitingMs) setPhase({ ...phase, again: "checked" });
          else timer = setTimeout(tick, promptTiming.recheckMs);
        },
        (e: unknown) => {
          if (!live) return;
          if (Date.now() - phase.at >= promptTiming.stillWaitingMs) {
            setPhase({ ...phase, again: "unchecked", why: isApiError(e) ? e.message : String(e) });
          } else timer = setTimeout(tick, promptTiming.recheckMs);
        },
      );
    };
    timer = setTimeout(tick, promptTiming.recheckMs);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [phase]);

  const label = (option: string) => prompt.options.find((o) => o.id === option)?.label ?? option;

  const choose = async (option: string) => {
    if (busy.current) return;
    busy.current = true;
    setPhase({ kind: "sending", option });
    try {
      await onAnswer(option);
      setPhase({ kind: "sent", option, at: Date.now(), again: "no" });
    } catch (e) {
      setPhase({ kind: "error", option, message: isApiError(e) ? e.message : String(e) });
      // no prompt to answer: the agent may have moved on already, so show where it stands
      if (isApiError(e) && e.code === "no_prompt") recheck.current().catch(() => undefined);
    } finally {
      busy.current = false;
    }
  };

  const locked = phase.kind === "sending" || (phase.kind === "sent" && phase.again === "no");

  return (
    <div className="prompt-answer" role="group" aria-label={`Question from ${who}`} data-prompt-answer="">
      {prompt.options.length === 1 && prompt.options[0]?.id === ANSWERED_IN_PANE && (
        <p className="prompt-answer__note muted">
          The app could not read the choices of this question, so this is the bottom of {who}'s screen.
        </p>
      )}
      <pre className="prompt-answer__text">{prompt.text}</pre>
      <div className="prompt-answer__choices">
        {prompt.options.map((o) => {
          const mine = phase.kind !== "idle" && phase.option === o.id;
          return (
            <Button
              key={o.id}
              tone={tone}
              solid={mine && phase.kind !== "error"}
              size="sm"
              disabled={locked}
              aria-busy={mine && phase.kind === "sending" ? true : undefined}
              onClick={() => void choose(o.id)}
            >
              {mine && phase.kind === "sending" ? `Sending “${o.label}”…` : o.label}
            </Button>
          );
        })}
      </div>
      <p className="prompt-answer__status muted" role="status">
        {phase.kind === "sending" && <>Sending your answer to {who}…</>}
        {phase.kind === "sent" &&
          (phase.again === "checked" ? (
            <>
              Sent “{label(phase.option)}”, but {who} is still at this question. You can choose again.
            </>
          ) : phase.again === "unchecked" ? (
            <>
              Sent “{label(phase.option)}”, but whether {who} carried on could not be checked ({phase.why}). You can choose
              again.
            </>
          ) : (
            <>
              Sent “{label(phase.option)}”. Waiting for {who} to carry on…
            </>
          ))}
      </p>
      {phase.kind === "error" && (
        <div className="prompt-answer__error action-result action-result--error" role="alert">
          <span>
            <strong>“{label(phase.option)}” was not sent.</strong> {phase.message}
          </span>
          <Button size="sm" onClick={() => void choose(phase.option)}>
            Try again
          </Button>
        </div>
      )}
    </div>
  );
}
