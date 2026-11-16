import { useId, useRef, useState, type KeyboardEvent } from "react";

/**
 * The message box. Enter sends, Shift+Enter starts a new line. The text is cleared only when
 * the send is accepted, so a refused message (no lead running) is never lost.
 */
export function Composer({ onSend, disabled }: { onSend: (text: string) => Promise<boolean>; disabled?: boolean }) {
  const [text, setText] = useState("");
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
      <button type="submit" className="button button--primary" disabled={disabled || text.trim() === ""}>
        Send
      </button>
      <p id={`${id}-hint`} className="muted chat-hint">
        Enter sends · Shift+Enter for a new line
      </p>
    </form>
  );
}
