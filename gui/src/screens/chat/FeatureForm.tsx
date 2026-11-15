import { useId, useRef, useState, type FormEvent } from "react";
import type { ChatRequest } from "../../api/types";
import { LeadUnavailable } from "../../components/useSendChat";
import type { Chat } from "./useChat";

export const EMPTY_REQUEST: ChatRequest = {
  feature: "",
  why: "",
  must: "",
  must_not: "",
  decided: "",
  done_when: "",
  mode: "",
};

type Errors = Partial<Record<"feature" | "why", string>>;

/**
 * The plan's feature request as a form. Feature and Why are required; nothing is sent until
 * they are filled. What was typed stays in the form when the send is refused.
 */
export function FeatureForm({
  values,
  onChange,
  chat,
  onSent,
  onCancel,
}: {
  values: ChatRequest;
  onChange: (v: ChatRequest) => void;
  chat: Chat;
  onSent: () => void;
  onCancel: () => void;
}) {
  const uid = useId();
  const [errors, setErrors] = useState<Errors>({});
  const [failure, setFailure] = useState<{ kind: "lead_unavailable" | "error"; text: string } | null>(null);
  const feature = useRef<HTMLInputElement>(null);
  const why = useRef<HTMLTextAreaElement>(null);
  const set = (k: keyof ChatRequest, v: string) => onChange({ ...values, [k]: v });
  const id = (k: string) => `${uid}-${k}`;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const next: Errors = {};
    if (values.feature.trim() === "") next.feature = "Say what the feature is.";
    if (values.why.trim() === "") next.why = "Say why it is wanted.";
    setErrors(next);
    if (next.feature) return feature.current?.focus();
    if (next.why) return why.current?.focus();
    setFailure(null);
    const r = await chat.sendNow({ kind: "feature_request", text: "", request: values });
    if (r.ok) {
      chat.adopt(r.message);
      chat.clearLeadProblem();
      onSent();
    } else {
      setFailure({ kind: r.kind, text: r.text });
    }
  };

  const area = (k: keyof ChatRequest, label: string, hint: string) => (
    <div className="field">
      <label htmlFor={id(k)}>{label}</label>
      <textarea id={id(k)} rows={2} value={values[k]} onChange={(e) => set(k, e.target.value)} aria-describedby={`${id(k)}-hint`} />
      <span id={`${id(k)}-hint`} className="muted chat-hint">
        {hint}
      </span>
    </div>
  );

  return (
    <form className="panel chat-form" aria-label="New feature request" onSubmit={(e) => void submit(e)} noValidate>
      <h2 className="panel-title">New feature request</h2>
      <p className="muted">The lead turns this into a plan for you to review. Only Feature and Why are required.</p>

      <div className="field">
        <label htmlFor={id("feature")}>
          Feature <span className="muted">(required)</span>
        </label>
        <input
          id={id("feature")}
          ref={feature}
          value={values.feature}
          onChange={(e) => set("feature", e.target.value)}
          required
          aria-required="true"
          aria-invalid={errors.feature ? true : undefined}
          aria-describedby={errors.feature ? `${id("feature")}-err` : undefined}
        />
        {errors.feature && (
          <span id={`${id("feature")}-err`} className="error-text" role="alert">
            {errors.feature}
          </span>
        )}
      </div>
      <div className="field">
        <label htmlFor={id("why")}>
          Why <span className="muted">(required)</span>
        </label>
        <textarea
          id={id("why")}
          ref={why}
          rows={2}
          value={values.why}
          onChange={(e) => set("why", e.target.value)}
          required
          aria-required="true"
          aria-invalid={errors.why ? true : undefined}
          aria-describedby={errors.why ? `${id("why")}-err` : undefined}
        />
        {errors.why && (
          <span id={`${id("why")}-err`} className="error-text" role="alert">
            {errors.why}
          </span>
        )}
      </div>
      {area("must", "Must", "What has to be true.")}
      {area("must_not", "Must not", "What must not change or break.")}
      {area("decided", "Already decided", "Choices that are made: the lead will not ask again.")}
      {area("done_when", "Done when", "How you will know it works.")}
      <div className="field">
        <label htmlFor={id("mode")}>Mode</label>
        <select id={id("mode")} value={values.mode} onChange={(e) => set("mode", e.target.value)}>
          <option value="">Let the lead suggest</option>
          <option value="worktree">worktree (each coder works apart)</option>
          <option value="shared">shared (one folder)</option>
        </select>
      </div>

      {failure?.kind === "lead_unavailable" && <LeadUnavailable text={failure.text} />}
      {failure?.kind === "error" && (
        <p className="action-result action-result--error" role="alert">
          {failure.text}
        </p>
      )}
      <div className="button-row">
        <button type="submit" className="button button--primary" disabled={chat.sending}>
          Send feature request
        </button>
        <button type="button" className="button" onClick={onCancel}>
          Close
        </button>
      </div>
    </form>
  );
}
