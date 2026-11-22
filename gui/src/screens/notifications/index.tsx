import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { TaggedItem } from "../../components/attentionStore";
import { useAttention } from "../../components/Attention";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { useToast } from "../../ui/Toast";
import { OpenItemCard, ResolvedRow } from "./ItemCard";
import { KIND_ORDER, kindLook } from "./kinds";
import "./notifications.css";

type Status = "open" | "resolved" | "all";

const STATUSES: { value: Status; label: string }[] = [
  { value: "open", label: "Open" },
  { value: "resolved", label: "Resolved" },
  { value: "all", label: "All" },
];

function matches(text: string, query: string): boolean {
  return text.toLowerCase().includes(query.trim().toLowerCase());
}

/** Screen 09: every open and locally resolved attention item, across projects. */
export function NotificationsScreen() {
  const { waitingItems, resolvedItems, resolveItem, clientFor, openItem } = useAttention();
  const toast = useToast();
  const [project, setProject] = useState("all");
  const [kind, setKind] = useState("all");
  const [status, setStatus] = useState<Status>("open");
  const [query, setQuery] = useState("");
  const [bulkPending, setBulkPending] = useState(false);
  const tabRefs = useRef<Map<Status, HTMLButtonElement>>(new Map());

  const handleTabKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const keys = STATUSES.map((s) => s.value);
    let i = keys.indexOf(status);
    if (e.key === "ArrowRight") i = (i + 1) % keys.length;
    else if (e.key === "ArrowLeft") i = (i - 1 + keys.length) % keys.length;
    else if (e.key === "Home") i = 0;
    else if (e.key === "End") i = keys.length - 1;
    else return;
    e.preventDefault();
    const next = keys[i]!;
    setStatus(next);
    tabRefs.current.get(next)?.focus();
  };

  const projectNames = useMemo(() => {
    const names = new Set<string>();
    for (const item of [...waitingItems, ...resolvedItems]) if (item.projectName) names.add(item.projectName);
    return [...names].sort((a, b) => a.localeCompare(b));
  }, [waitingItems, resolvedItems]);

  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const item of waitingItems) c[item.kind] = (c[item.kind] ?? 0) + 1;
    return c;
  }, [waitingItems]);

  const inScope = (item: TaggedItem) =>
    (project === "all" || item.projectName === project) && (kind === "all" || item.kind === kind);

  const inSearch = (item: TaggedItem) => {
    if (query.trim() === "") return true;
    const look = kindLook(item.kind);
    return matches(
      [item.title, item.detail, item.prompt?.text, item.task, item.agent, item.projectName, look.label, item.kind]
        .filter(Boolean)
        .join(" "),
      query,
    );
  };

  const openRows = waitingItems.filter((i) => inScope(i) && inSearch(i));
  const resolvedRows = resolvedItems.filter((i) => inScope(i) && inSearch(i));
  const statusScope = [...waitingItems, ...resolvedItems].filter((i) => inScope(i));
  const rows = status === "open" ? openRows : status === "resolved" ? resolvedRows : [...openRows, ...resolvedRows];
  const projectsInScope = waitingItems.filter((i) => inScope(i));
  const unresolved = waitingItems.length;
  const holds = waitingItems.filter((i) => i.kind === "held" && i.task && clientFor(i.sourceKey));

  const grouped = KIND_ORDER.map((k) => ({ kind: k, rows: openRows.filter((i) => i.kind === k) })).filter((g) => g.rows.length > 0);
  const unknown = openRows.filter((i) => !KIND_ORDER.includes(i.kind as (typeof KIND_ORDER)[number]));

  const hasFilter = project !== "all" || kind !== "all" || query.trim() !== "" || status !== "open";
  const reset = () => {
    setProject("all");
    setKind("all");
    setStatus("open");
    setQuery("");
  };

  const approveHolds = async () => {
    setBulkPending(true);
    let done = 0;
    for (const item of holds) {
      const client = clientFor(item.sourceKey);
      if (!client || !item.task) continue;
      try {
        await client.approve(item.task);
        resolveItem(item, "Approved");
        done += 1;
      } catch {
        // The card keeps its own error; bulk reports the count that did work.
      }
    }
    setBulkPending(false);
    toast(`Holds approved: ${done} item${done === 1 ? "" : "s"}`, { tone: "amber", icon: "check" });
  };

  return (
    <section className="screen nc" aria-labelledby="nc-title">
      <header className="nc__head">
        <span className="nc__icon" aria-hidden="true">
          <Icon name="bell" size={34} />
        </span>
        <div className="nc__who">
          <h1 className="nc__title" id="nc-title" aria-label="Notifications / Attention Center">
            Attention Center ({unresolved} Unresolved)
          </h1>
          <p className="nc__sub">Cross-project attention, questions, risk holds and blocked prompts.</p>
        </div>
        <div className="nc__head-actions">
          <Button
            tone="amber"
            onClick={() => void approveHolds()}
            disabled={holds.length === 0 || bulkPending}
            title={holds.length === 0 ? "No risk holds are waiting" : undefined}
          >
            <Icon name="warning" />
            {bulkPending ? "Approving…" : `Approve all holds (${holds.length})`}
          </Button>
          <Button unavailable="Not available from the app yet: page-local alerts are not part of the API.">
            <Icon name="sync" />
            Restore local alerts
          </Button>
        </div>
      </header>

      <Card as="section" className="nc__filters" aria-label="Filters">
        {projectNames.length > 1 && (
          <div className="nc__chips" role="group" aria-label="Project">
            <button
              type="button"
              className="fchip"
              aria-pressed={project === "all"}
              onClick={() => setProject("all")}
            >
              All Projects <span className="mono">({unresolved})</span>
            </button>
            {projectNames.map((name) => (
              <button
                key={name}
                type="button"
                className="fchip"
                aria-pressed={project === name}
                onClick={() => setProject(name)}
              >
                {name}{" "}
                <span className="mono">({waitingItems.filter((i) => i.projectName === name).length})</span>
              </button>
            ))}
          </div>
        )}

        <div className="nc__chips" role="group" aria-label="Kind">
          <button type="button" className="fchip" aria-pressed={kind === "all"} onClick={() => setKind("all")}>
            All kinds
          </button>
          {KIND_ORDER.map((k) => (
            <button
              key={k}
              type="button"
              className="fchip"
              aria-pressed={kind === k}
              onClick={() => setKind(k)}
            >
              {kindLook(k).chip} <span className="mono">({counts[k] ?? 0})</span>
            </button>
          ))}
        </div>

        <div className="nc__searchrow">
          <label className="nc__search">
            <span className="visually-hidden">Search notifications</span>
            <Icon name="search" />
            <input
              className="nc__search-input"
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search by milestone, agent or message…"
              autoComplete="off"
            />
          </label>
          <div className="seg nc__tabs" role="tablist" aria-label="Status" onKeyDown={handleTabKeyDown}>
            {STATUSES.map((tab) => {
              const n = tab.value === "all" ? statusScope.length : statusScope.filter((i) => ("resolvedAt" in i ? "resolved" : "open") === tab.value).length;
              const selected = status === tab.value;
              return (
                <button
                  key={tab.value}
                  id={`status-tab-${tab.value}`}
                  ref={(el) => {
                    if (el) tabRefs.current.set(tab.value, el);
                    else tabRefs.current.delete(tab.value);
                  }}
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  aria-controls="status-panel"
                  tabIndex={selected ? 0 : -1}
                  onClick={() => setStatus(tab.value)}
                >
                  {tab.label} <span className="tabcount">{n}</span>
                </button>
              );
            })}
          </div>
        </div>
      </Card>

      <div className="nc__grid">
        <section className="nc__listwrap" aria-labelledby="nc-list-title">
          <h2 className="visually-hidden" id="nc-list-title">
            Notifications
          </h2>

          <div
            role="tabpanel"
            id="status-panel"
            aria-labelledby={`status-tab-${status}`}
            tabIndex={-1}
          >
            {rows.length === 0 ? (
              <Card as="div" className="nc-empty">
                <Icon name="inbox" />
                <span>
                  {status === "open" && !hasFilter
                    ? "All clear. Nothing needs your attention."
                    : "No notifications match these filters."}
                </span>
                {hasFilter && (
                  <Button size="sm" onClick={reset}>
                    Reset filters
                  </Button>
                )}
              </Card>
            ) : (
              <div className="nc__list">
                {openRows.length > 0 && status !== "resolved" && (
                  <>
                    {grouped.map((group) => {
                      const look = kindLook(group.kind);
                      return (
                        <section className="nc__group" key={group.kind} aria-labelledby={`nc-kind-${group.kind}`}>
                          <h2 className="nc__group-title" id={`nc-kind-${group.kind}`}>
                            <Icon name={look.icon} />
                            {look.chip}
                            <Badge tone={look.tone}>{group.rows.length}</Badge>
                          </h2>
                          <ul className="nc__list-ul">
                            {group.rows.map((item) => {
                              const client = clientFor(item.sourceKey);
                              return client ? (
                                <OpenItemCard
                                  key={item.uid}
                                  item={item}
                                  client={client}
                                  onReview={openItem}
                                  onResolved={resolveItem}
                                />
                              ) : null;
                            })}
                          </ul>
                        </section>
                      );
                    })}
                    {unknown.length > 0 && (
                      <section className="nc__group" aria-labelledby="nc-kind-other">
                        <h2 className="nc__group-title" id="nc-kind-other">
                          <Icon name="bell" />
                          Other
                          <Badge>{unknown.length}</Badge>
                        </h2>
                        <ul className="nc__list-ul">
                          {unknown.map((item) => {
                            const client = clientFor(item.sourceKey);
                            return client ? (
                              <OpenItemCard key={item.uid} item={item} client={client} onReview={openItem} onResolved={resolveItem} />
                            ) : null;
                          })}
                        </ul>
                      </section>
                    )}
                  </>
                )}

                {resolvedRows.length > 0 && status !== "open" && (
                  <section className="nc__group" aria-labelledby="nc-resolved-title">
                    <h2 className="nc__group-title" id="nc-resolved-title">
                      <Icon name="check" />
                      Resolved history
                      <Badge tone="emerald">{resolvedRows.length}</Badge>
                    </h2>
                    <ul className="nc__done-list">
                      {resolvedRows.map((item) => (
                        <ResolvedRow key={item.uid} item={item} />
                      ))}
                    </ul>
                  </section>
                )}
              </div>
            )}
          </div>
        </section>

        <aside className="nc__side" aria-label="Summary and resolved history">
          <Card as="section" className="nc__panel" aria-labelledby="nc-sum-title">
            <h2 className="nc__panel-title" id="nc-sum-title">
              <Icon name="bell" />
              Open by kind ({unresolved})
            </h2>
            <ul className="nc__sum">
              {KIND_ORDER.map((k) => {
                const n = waitingItems.filter((i) => i.kind === k).length;
                return (
                  <li key={k}>
                    <button
                      type="button"
                      onClick={() => {
                        setKind(k);
                        setStatus("open");
                      }}
                    >
                      <span className={`dot dot--${kindLook(k).tone}`} aria-hidden="true" />
                      {kindLook(k).chip}
                      <span className="n">{n}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
            {projectsInScope.length > 0 && <p className="muted nc__scope">{projectsInScope.length} open item{projectsInScope.length === 1 ? "" : "s"} in this view.</p>}
          </Card>

          <Card as="section" className="nc__panel" aria-labelledby="nc-hist-title">
            <h2 className="nc__panel-title" id="nc-hist-title">
              <Icon name="check" />
              Resolved history ({resolvedItems.length})
            </h2>
            {resolvedItems.length === 0 ? (
              <p className="muted">Nothing resolved yet.</p>
            ) : (
              <ul className="nc__hist">
                {resolvedItems.slice(0, 8).map((item) => (
                  <li key={item.uid}>
                    <span className="mono">
                      [{item.task ?? "Plan"}] {item.title}
                    </span>
                    <span>{item.resolution}</span>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </aside>
      </div>
    </section>
  );
}
