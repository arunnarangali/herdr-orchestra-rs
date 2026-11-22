import { useId, useRef, useState, type KeyboardEvent } from "react";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { FEAT_COMMAND, FEAT_HELP, suggestsFeat } from "./featureCommand";

/**
 * The message box. Enter sends, Shift+Enter starts a new line. The text is cleared only when
 * the send is accepted, so a refused message (no lead running) is never lost.
 */
export function Composer({
  onSend,
  onNewRequest,
  disabled,
  text: kept,
  onText,
  problem,
}: {
  onSend: (text: string) => Promise<boolean>;
  onNewRequest?: () => void;
  disabled?: boolean;
  /** The unsent text, when the screen keeps it (so it survives leaving the page). */
  text?: string;
  onText?: (text: string) => void;
  /** Why the last send was not made (said in plain words next to the box). */
  problem?: string | null;
}) {
  const [own, setOwn] = useState("");
  const text = kept ?? own;
  const setText = onText ?? setOwn;
  const id = useId();
  const box = useRef<HTMLTextAreaElement>(null);

  const suggest = suggestsFeat(text);
  const complete = () => {
    setText(`${FEAT_COMMAND} `);
    box.current?.focus();
  };
  const submit = async () => {
    const t = text.trim();
    if (t === "" || disabled) return;
    if (await onSend(t)) setText("");
    box.current?.focus();
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      // a command still being typed is completed, not sent
      if (suggest) complete();
      else void submit();
    }
  };

  return (
    <form
      className="chat-composer"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      {onNewRequest && (
        <Button
          className="composer__new"
          icon
          aria-label="Open feature request form"
          title="New feature request"
          onClick={onNewRequest}
          disabled={disabled}
        >
          <Icon name="plus" />
        </Button>
      )}
      <label htmlFor={id} className="visually-hidden">
        Message to the lead
      </label>
      <textarea
        id={id}
        ref={box}
        rows={2}
        value={text}
        placeholder="Message the lead…"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={onKey}
        aria-describedby={`${id}-hint${problem ? ` ${id}-problem` : ""}`}
        aria-invalid={problem ? true : undefined}
      />
      {suggest && (
        <ul className="chat-suggest" role="listbox" aria-label="Commands">
          <li role="option" aria-selected="true">
            <button type="button" className="chat-suggest__item" onClick={complete}>
              <code>{FEAT_COMMAND}</code>
              <span className="muted">{FEAT_HELP}</span>
            </button>
          </li>
        </ul>
      )}
      <button type="submit" className="button button--primary composer__send" disabled={disabled || text.trim() === ""} aria-label="Send">
        Send
      </button>
      {problem && (
        <p id={`${id}-problem`} className="chat-problem" role="alert">
          {problem}
        </p>
      )}
      <p id={`${id}-hint`} className="muted chat-hint">
        Enter sends · Shift+Enter for a new line
      </p>
    </form>
  );
}
