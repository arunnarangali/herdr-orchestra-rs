import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useApi } from "../../app/connection";
import type { AgentOutputResponse } from "../../api/types";
import { ApiError } from "../../api/errors";
import { Modal } from "../../ui/Modal";

export interface AgentOutputModalProps {
  agentName: string;
  onClose: () => void;
  /** Poll interval in milliseconds; defaults to 2000. */
  pollIntervalMs?: number;
}

const LINE_OPTIONS = [50, 100, 200, 500, 1000, 2000, 5000] as const;

export function AgentOutputModal({
  agentName,
  onClose,
  pollIntervalMs = 2000,
}: AgentOutputModalProps) {
  const api = useApi();
  const [lines, setLines] = useState<number>(200);
  const [paused, setPaused] = useState<boolean>(false);
  const [search, setSearch] = useState<string>("");
  const [data, setData] = useState<AgentOutputResponse | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [loading, setLoading] = useState<boolean>(true);
  const [isScrolledUp, setIsScrolledUp] = useState<boolean>(false);

  const preRef = useRef<HTMLPreElement>(null);
  const searchInputId = useId();
  const linesSelectId = useId();

  const fetchOutput = useCallback(
    async (count: number) => {
      try {
        const res = await api.agentOutput(agentName, { lines: count });
        setData(res);
        setError(null);
      } catch (e) {
        setError(
          e instanceof ApiError
            ? e
            : new ApiError("read_failed", e instanceof Error ? e.message : String(e), 0),
        );
      } finally {
        setLoading(false);
      }
    },
    [api, agentName],
  );

  // Initial fetch and on lines change
  useEffect(() => {
    setLoading(true);
    void fetchOutput(lines);
  }, [fetchOutput, lines]);

  // Polling while open and not paused
  useEffect(() => {
    if (paused) return;
    const timer = setInterval(() => {
      void fetchOutput(lines);
    }, pollIntervalMs);
    return () => clearInterval(timer);
  }, [fetchOutput, lines, paused, pollIntervalMs]);

  // Auto-scroll to bottom when new data arrives, unless the user has scrolled up
  useEffect(() => {
    if (!data) return;
    const el = preRef.current;
    if (!el) return;
    if (!isScrolledUp) {
      el.scrollTop = el.scrollHeight;
    }
  }, [data, isScrolledUp]);

  const handleScroll = () => {
    const el = preRef.current;
    if (!el) return;
    // User is near bottom if within 30px of bottom
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
    setIsScrolledUp(!atBottom);
  };

  const scrollToBottom = () => {
    const el = preRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
      setIsScrolledUp(false);
    }
  };

  // Render text with search highlighting
  const renderContent = () => {
    if (loading && !data) {
      return (
        <p role="status" className="muted ag-output-status">
          Loading output for {agentName}…
        </p>
      );
    }
    if (error) {
      return (
        <div role="alert" className="ag-output-error">
          <p className="st-msg st-msg--error">
            {error.code === "agent_not_found"
              ? `Agent "${agentName}" not found.`
              : `Cannot read output for ${agentName}: ${error.message}`}
          </p>
          <button
            type="button"
            className="button"
            onClick={() => {
              setLoading(true);
              void fetchOutput(lines);
            }}
          >
            Retry
          </button>
        </div>
      );
    }
    const text = data?.text ?? "";
    if (text.length === 0) {
      return <p className="muted ag-output-empty">No recent output recorded for {agentName}.</p>;
    }

    const query = search.trim().toLowerCase();
    if (!query) {
      return (
        <pre
          ref={preRef}
          className="ag-output-pre"
          tabIndex={0}
          onScroll={handleScroll}
          aria-label={`Live output for ${agentName}`}
        >
          {text}
        </pre>
      );
    }

    // Split text and highlight search matches
    const parts: ReactNode[] = [];
    let lastIndex = 0;
    const lower = text.toLowerCase();
    let matchIndex = lower.indexOf(query, lastIndex);
    let matchCount = 0;

    while (matchIndex !== -1) {
      matchCount++;
      if (matchIndex > lastIndex) {
        parts.push(text.slice(lastIndex, matchIndex));
      }
      parts.push(
        <mark key={matchIndex} className="ag-output-hit">
          {text.slice(matchIndex, matchIndex + query.length)}
        </mark>,
      );
      lastIndex = matchIndex + query.length;
      matchIndex = lower.indexOf(query, lastIndex);
    }
    if (lastIndex < text.length) {
      parts.push(text.slice(lastIndex));
    }

    return (
      <>
        <div className="ag-output-search-summary" role="status">
          {matchCount === 0 ? "No matches found" : `${matchCount} match${matchCount === 1 ? "" : "es"} found`}
        </div>
        <pre
          ref={preRef}
          className="ag-output-pre"
          tabIndex={0}
          onScroll={handleScroll}
          aria-label={`Live output for ${agentName}`}
        >
          {parts}
        </pre>
      </>
    );
  };

  return (
    <Modal
      open={true}
      title={`Live Output: ${agentName}`}
      onClose={onClose}
      initialFocus={`#${searchInputId}`}
    >
      <div className="ag-output-dialog">
        <div className="ag-output-toolbar">
          <div className="ag-output-controls">
            <div className="ag-output-field">
              <label htmlFor={linesSelectId}>Lines:</label>
              <select
                id={linesSelectId}
                value={lines}
                onChange={(e) => setLines(Number(e.target.value))}
              >
                {LINE_OPTIONS.map((opt) => (
                  <option key={opt} value={opt}>
                    {opt}
                  </option>
                ))}
              </select>
            </div>

            <button
              type="button"
              className={paused ? "button button--primary" : "button"}
              onClick={() => setPaused((p) => !p)}
              aria-pressed={paused}
            >
              {paused ? "Resume refresh" : "Pause refresh"}
            </button>

            <span className={paused ? "badge badge--warning" : "badge badge--success"}>
              {paused ? "Paused" : "Live"}
            </span>

            {data && <span className="muted ag-output-meta">{data.lines} lines read</span>}
          </div>

          <div className="ag-output-search">
            <label htmlFor={searchInputId} className="visually-hidden">
              Search inside output
            </label>
            <input
              id={searchInputId}
              type="search"
              placeholder="Search in output…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              spellCheck={false}
            />
          </div>
        </div>

        <div className="ag-output-body">
          {renderContent()}
          {isScrolledUp && (
            <button
              type="button"
              className="button button--small ag-output-scroll-btn"
              onClick={scrollToBottom}
              aria-label="Scroll to newest output"
            >
              ↓ Scroll to bottom
            </button>
          )}
        </div>

        <div className="modal-actions">
          <button type="button" className="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </Modal>
  );
}
