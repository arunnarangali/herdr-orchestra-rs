import { useId, useRef, useState, type KeyboardEvent } from "react";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";

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
}: {
  onSend: (text: string) => Promise<boolean>;
  onNewRequest?: () => void;
  disabled?: boolean;
  /** The unsent text, when the screen keeps it (so it survives leaving the page). */
  text?: string;
  onText?: (text: string) => void;
}) {
  const [own, setOwn] = useState("");
  const text = kept ?? own;
  const setText = onText ?? setOwn;
  const id = useId();
  const box = useRef<HTMLTextAreaElement>(null);

  const submit = async () => {
    const t = text.trim();
    if (t === "" || disabled) return;
    if (await onSend(t)) setText("");
    box.current?.focus();
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void submit();
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
        aria-describedby={`${id}-hint`}
      />
      <button type="submit" className="button button--primary composer__send" disabled={disabled || text.trim() === ""} aria-label="Send">
        Send
      </button>
      <p id={`${id}-hint`} className="muted chat-hint">
        Enter sends · Shift+Enter for a new line
      </p>
    </form>
  );
}
