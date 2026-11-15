import type { CheckInfo, CoderResult, HistoryEntry, ReviewInfo, TaskDetailResponse } from "../api/types";
import { StatusBadge } from "../ui/StatusBadge";
import { formatTime } from "./format";
import "./components.css";

// Everything here is text from agents, reviews or the repository: it is rendered as React text
// nodes, never as HTML.

export function HistoryList({ history }: { history: HistoryEntry[] }) {
  if (history.length === 0) return <p className="muted">No transitions yet.</p>;
  return (
    <ol className="history">
      {history.map((h, i) => (
        <li key={`${h.ts}-${i}`}>
          <time className="muted">{formatTime(h.ts)}</time> <StatusBadge state={h.from} /> →{" "}
          <StatusBadge state={h.to} /> <span>{h.why}</span>
          <span className="muted">
            {" "}
            attempt {h.attempt}
            {h.worker ? `, ${h.worker}` : ""}
            {h.model ? `, ${h.model}` : ""}
            {h.forced ? ", forced" : ""}
          </span>
        </li>
      ))}
    </ol>
  );
}

export function ChecksList({ checks }: { checks: CheckInfo[] }) {
  if (checks.length === 0) return <p className="muted">No checks recorded.</p>;
  return (
    <ul className="checks">
      {checks.map((c, i) => (
        <li key={`${c.ts}-${i}`}>
          <span className={c.ok ? "badge badge--success" : "badge badge--danger"}>{c.ok ? "passed" : "failed"}</span>{" "}
          in the {c.where} <span className="muted">({c.secs.toFixed(1)}s, {formatTime(c.ts)})</span>
        </li>
      ))}
    </ul>
  );
}

export function ReviewsPanel({ reviews }: { reviews: ReviewInfo[] }) {
  if (reviews.length === 0) return <p className="muted">No reviews yet.</p>;
  return (
    <div className="reviews">
      {reviews.map((r) => (
        <article key={r.round} className="review" aria-label={`Review round ${r.round}`}>
          <h4>
            Round {r.round}
            {r.outcome && <span className="badge badge--info"> {r.outcome}</span>}
          </h4>
          {r.error && <p className="error-text">The review file is unusable: {r.error}</p>}
          {r.criteria.length > 0 && (
            <ul className="criteria">
              {r.criteria.map((c) => (
                <li key={c.id}>
                  <span className={c.met ? "badge badge--success" : "badge badge--danger"}>{c.met ? "met" : "not met"}</span>{" "}
                  <strong>{c.id}</strong> {c.note}
                </li>
              ))}
            </ul>
          )}
          <Findings title="Blockers" items={r.blockers} />
          <Findings title="Suggestions" items={r.suggestions} />
        </article>
      ))}
    </div>
  );
}

function Findings({ title, items }: { title: string; items: ReviewInfo["blockers"] }) {
  if (items.length === 0) return null;
  return (
    <section>
      <h5>
        {title} ({items.length})
      </h5>
      <ul className="findings">
        {items.map((f, i) => (
          <li key={i}>
            <code>
              {f.file}
              {f.line ? `:${f.line}` : ""}
            </code>{" "}
            {f.criterion && <span className="muted">({f.criterion}) </span>}
            <div>{f.problem}</div>
            <div className="muted">{f.why}</div>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function ResultsPanel({ results }: { results: CoderResult[] }) {
  if (results.length === 0) return <p className="muted">The coder has not written a result yet.</p>;
  return (
    <div className="results">
      {results.map((r) => (
        <article key={r.attempt} className="result" aria-label={`Result of attempt ${r.attempt}`}>
          <h4>
            Attempt {r.attempt}
            {r.status && <span className="badge badge--info"> {r.status}</span>}
          </h4>
          {r.error && <p className="error-text">The result file is unusable: {r.error}</p>}
          {r.commit && <p>Commit: {r.commit}</p>}
          {r.check && (
            <p>
              Check {r.check.ran ? (r.check.passed ? "passed" : "failed") : "not run"}: {r.check.summary}
            </p>
          )}
          {r.criteria.length > 0 && (
            <ul>
              {r.criteria.map((c) => (
                <li key={c.id}>
                  <strong>{c.id}</strong> {c.met ? "met" : "not met"}: {c.evidence}
                </li>
              ))}
            </ul>
          )}
          {r.questions.length > 0 && (
            <>
              <h5>Questions</h5>
              <ul>
                {r.questions.map((q, i) => (
                  <li key={i}>{q}</li>
                ))}
              </ul>
            </>
          )}
        </article>
      ))}
    </div>
  );
}

export function FilesPanel({ changed, outside }: { changed: string[]; outside: string[] }) {
  return (
    <div className="files">
      <h4>Files changed ({changed.length})</h4>
      {changed.length === 0 ? (
        <p className="muted">None yet.</p>
      ) : (
        <ul>
          {changed.map((f) => (
            <li key={f}>
              <code>{f}</code>
            </li>
          ))}
        </ul>
      )}
      <h4>Outside the listed files ({outside.length})</h4>
      {outside.length === 0 ? (
        <p className="muted">None.</p>
      ) : (
        <ul>
          {outside.map((f) => (
            <li key={f}>
              <code>{f}</code>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** A milestone opened: history, checks, reviews, the coder's results, files and logs. */
export function TaskDetail({ detail, logs }: { detail: TaskDetailResponse; logs: string[] | null }) {
  return (
    <div className="task-detail">
      <section aria-labelledby="td-history">
        <h3 id="td-history">Transition history</h3>
        <HistoryList history={detail.history} />
      </section>
      <section aria-labelledby="td-checks">
        <h3 id="td-checks">Checks</h3>
        <ChecksList checks={detail.checks} />
      </section>
      <section aria-labelledby="td-reviews">
        <h3 id="td-reviews">Reviews</h3>
        <ReviewsPanel reviews={detail.reviews} />
      </section>
      <section aria-labelledby="td-results">
        <h3 id="td-results">Coder's result</h3>
        <ResultsPanel results={detail.results} />
      </section>
      <section aria-labelledby="td-files">
        <h3 id="td-files">Files</h3>
        <FilesPanel changed={detail.files_changed} outside={detail.files_outside_likely} />
      </section>
      <section aria-labelledby="td-logs">
        <h3 id="td-logs">Logs</h3>
        {logs === null ? (
          <p className="muted">Loading logs…</p>
        ) : logs.length === 0 ? (
          <p className="muted">No log lines for this milestone.</p>
        ) : (
          <pre className="log" tabIndex={0} aria-label="Log lines">
            {logs.join("\n")}
          </pre>
        )}
      </section>
    </div>
  );
}
