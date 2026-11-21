import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { screens } from "../../app/routes";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon, type IconName } from "../../ui/Icon";
import { useToast } from "../../ui/Toast";
import { CLI, ENV, EXIT, GUARANTEES } from "./content";
import "./help.css";

interface Shortcut {
  keys: string[];
  /** How the keys combine: pressed together (`+`) or one after the other (`then`). */
  joiner: "+" | "then" | "";
  title: string;
  sub: string;
}

/** The shortcuts the app really has: the `g` chords come from the screen registry. */
function shortcuts(): Shortcut[] {
  return [
    { keys: ["Ctrl", "K"], joiner: "+", title: "Open Command Palette", sub: "(go to a screen, run an action)" },
    ...screens
      .filter((s) => s.shortcut)
      .map((s) => ({ keys: ["G", (s.shortcut ?? "").toUpperCase()], joiner: "then" as const, title: `Go to ${s.label}`, sub: "" })),
    { keys: ["?"], joiner: "", title: "Show all shortcuts", sub: "(the shortcut list)" },
    { keys: ["/"], joiner: "", title: "Search this page", sub: "(focus the help search box)" },
    { keys: ["Esc"], joiner: "", title: "Close a dialog or menu", sub: "(or clear this search)" },
  ];
}

const norm = (s: string) => s.toLowerCase();

function CopyGlyph() {
  return (
    <svg className="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      <path d="M11 9h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2zM5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1" />
    </svg>
  );
}

function CopyButton({ text }: { text: string }) {
  const toast = useToast();
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
      toast(`Copied: ${text}`, { tone: "emerald" });
    } catch {
      toast(`Copying is blocked here. Select it and copy: ${text}`, { tone: "amber", icon: "warning" });
    }
  };
  return (
    <button type="button" className="copybtn" data-copied={copied} aria-label={`Copy ${text}`} title="Copy" onClick={() => void copy()}>
      {copied ? <Icon name="check" /> : <CopyGlyph />}
    </button>
  );
}

function Section({ id, icon, title, sub, green, children }: { id: string; icon: IconName; title: string; sub?: string; green?: boolean; children: ReactNode }) {
  return (
    <Card as="section" tone={green ? "emerald" : undefined} className="hsec" aria-labelledby={`hp-${id}`}>
      <div className="hsec__head">
        <span className="hsec__icon">
          <Icon name={icon} />
        </span>
        <div>
          <h2 className="hsec__title" id={`hp-${id}`}>
            {title}
          </h2>
          {sub && <p className="hsec__sub">{sub}</p>}
        </div>
      </div>
      {children}
    </Card>
  );
}

/** Screen 11: the CLI reference, exit codes, tunables, keyboard shortcuts and guarantees, searchable. */
export function HelpScreen() {
  const [query, setQuery] = useState("");
  const [closed, setClosed] = useState<Set<string>>(() => new Set());
  const input = useRef<HTMLInputElement>(null);
  const inputId = useId();
  const q = norm(query.trim());
  const hit = (...parts: (string | number)[]) => !q || norm(parts.join(" ")).includes(q);

  // `/` focuses the search, unless the user is typing somewhere or a dialog is open.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      const typing = t && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));
      if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey || typing || document.querySelector('[aria-modal="true"]')) return;
      e.preventDefault();
      input.current?.focus();
      input.current?.select();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const groups = CLI.map((g) => ({ ...g, rows: g.rows.filter(([c, d]) => hit(c, d, g.title, g.sub, "cli command orch")) })).filter((g) => g.rows.length);
  const exits = EXIT.filter(([c, t, d]) => hit("exit code", c, t, d));
  const envs = ENV.filter(([k, v, d]) => hit("environment tunable variable", k, v, d));
  const keys = shortcuts().filter((k) => hit("keyboard shortcut", k.keys.join(" "), k.title, k.sub));
  const guarantees = GUARANTEES.filter(([t, d]) => hit("architecture guarantee", t, d));
  const count = groups.reduce((n, g) => n + g.rows.length, 0) + exits.length + envs.length + keys.length + guarantees.length;

  const toggle = (id: string) =>
    setClosed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const clear = () => {
    setQuery("");
    input.current?.focus();
  };

  return (
    <section className="screen hp" aria-labelledby="screen-title">
      <header className="hp__head">
        <span className="hp__icon">
          <Icon name="help" />
        </span>
        <div>
          <h1 id="screen-title" className="hp__title">
            Help
          </h1>
          <p className="hp__sub">Help &amp; Reference: CLI commands, exit codes, keyboard shortcuts and the core architecture rules of Herdr Orchestra.</p>
        </div>
      </header>

      <Card className="hp__search" role="search">
        <Icon name="search" size={18} />
        <label className="visually-hidden" htmlFor={inputId}>
          Search help
        </label>
        <input
          id={inputId}
          ref={input}
          className="hp__q"
          type="search"
          autoComplete="off"
          spellCheck={false}
          placeholder="Search CLI commands, exit codes, keyboard shortcuts, or architecture rules…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && query) {
              e.stopPropagation();
              setQuery("");
            }
          }}
        />
        <kbd title="Press / to focus search">/</kbd>
      </Card>
      <p className="visually-hidden" aria-live="polite">
        {q ? (count ? `${count} result${count === 1 ? "" : "s"}` : "No results") : ""}
      </p>

      {count === 0 ? (
        <Card as="section" className="hp__none" aria-label="No matches">
          <p>
            <strong>
              No matches for <span className="mono">{query.trim()}</span>
            </strong>
          </p>
          <p className="muted">
            Try a command name such as <span className="mono">orch status</span>, an exit code like <span className="mono">3</span>, or a word
            such as <span className="mono">lease</span>.
          </p>
          <Button size="sm" onClick={clear}>
            Clear search
          </Button>
        </Card>
      ) : (
        <div className="hp__grid">
          <div className="hp__col hp__col--wide">
            {groups.length > 0 && (
              <Section id="cli" icon="keyboard" title="Core CLI Commands (orch)" sub="Essential commands for orchestration and control.">
                <div className="hsec__body">
                  {groups.map((g) => {
                    const open = q !== "" || !closed.has(g.id);
                    return (
                      <div className="grp" key={g.id}>
                        <button type="button" className="grp__btn" aria-expanded={open} aria-controls={`hp-grp-${g.id}`} onClick={() => toggle(g.id)}>
                          <span className="grp__txt">
                            <span className="grp__title">{g.title}</span>
                            <span className="grp__sub">{g.sub}</span>
                          </span>
                          <span className="grp__chev" aria-hidden="true">
                            ▾
                          </span>
                        </button>
                        <div className="grp__panel" id={`hp-grp-${g.id}`} hidden={!open}>
                          {g.rows.map(([c, d]) => (
                            <div className="cli" key={c}>
                              <pre>
                                <code>
                                  <span className="cmd">{c}</span>
                                  <span className="cm">{d}</span>
                                </code>
                              </pre>
                              <CopyButton text={c} />
                            </div>
                          ))}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </Section>
            )}
          </div>
          <div className="hp__col">
            {exits.length > 0 && (
              <Section id="exit" icon="grid" title="Exit Codes (Deterministic)" sub="Process exit codes and their meaning.">
                <table className="table">
                  <caption className="visually-hidden">Exit codes</caption>
                  <thead>
                    <tr>
                      <th scope="col">Code</th>
                      <th scope="col">Meaning</th>
                    </tr>
                  </thead>
                  <tbody>
                    {exits.map(([c, t, d]) => (
                      <tr key={c}>
                        <td>
                          <span className={`codebox codebox--${c}`}>{c}</span>
                        </td>
                        <td className="meaning">
                          <strong>{t}</strong> - {d}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </Section>
            )}
            {envs.length > 0 && (
              <Section id="env" icon="gear" title="Environment Tunables" sub="Common environment variables and their defaults.">
                <table className="table">
                  <caption className="visually-hidden">Environment variables</caption>
                  <thead>
                    <tr>
                      <th scope="col">Variable</th>
                      <th scope="col">Default</th>
                      <th scope="col">
                        <span className="visually-hidden">Copy</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {envs.map(([k, v, d]) => (
                      <tr key={k}>
                        <td>
                          <code className="envvar" title={d}>
                            {k}
                          </code>
                          <div className="act-sub">{d}</div>
                        </td>
                        <td>
                          <code className="envval">= {v}</code>
                        </td>
                        <td className="act">
                          <CopyButton text={`${k}=${v}`} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </Section>
            )}
          </div>
          <div className="hp__col">
            {keys.length > 0 && (
              <Section id="keys" icon="keyboard" title="Keyboard Shortcuts" sub="Fast navigation and common actions in the app.">
                <table className="table">
                  <caption className="visually-hidden">Keyboard shortcuts</caption>
                  <thead>
                    <tr>
                      <th scope="col">Shortcut</th>
                      <th scope="col">Action</th>
                    </tr>
                  </thead>
                  <tbody>
                    {keys.map((k) => (
                      <tr key={k.title}>
                        <td>
                          <span className="keys">
                            {k.keys.map((key, i) => (
                              <span key={key} className="keys__part">
                                {i > 0 && k.joiner && <span>{k.joiner}</span>}
                                <kbd>{key}</kbd>
                              </span>
                            ))}
                          </span>
                        </td>
                        <td>
                          <div className="act-title">{k.title}</div>
                          {k.sub && <div className="act-sub">{k.sub}</div>}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </Section>
            )}
            {guarantees.length > 0 && (
              <Section id="arch" icon="check" title="Architecture Guarantees" green>
                <ul className="guar">
                  {guarantees.map(([t, d]) => (
                    <li key={t}>
                      <Icon name="check" />
                      <div>
                        <strong>{t}</strong> <span className="muted">- {d}</span>
                      </div>
                    </li>
                  ))}
                </ul>
              </Section>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
