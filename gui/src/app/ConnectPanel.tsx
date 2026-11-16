import { useId, useState, type FormEvent } from "react";
import { normalizeUrl } from "../api/client";
import { useConnection } from "./connection";
import { useProjects } from "./projects";

function validUrl(input: string): boolean {
  try {
    const u = new URL(normalizeUrl(input));
    return (u.protocol === "http:" || u.protocol === "https:") && u.hostname !== "";
  } catch {
    return false;
  }
}

/**
 * How the user picks a server: in the desktop app, a project folder (the shell starts
 * `orch serve` for it); anywhere, the address and token of a server that is already running.
 * The token lives in this component's state and the connection's memory only: it is never
 * stored, logged or shown again.
 */
export function ConnectPanel() {
  const { shell, connect, credentials } = useConnection();
  const { multi, openFolder } = useProjects();
  const urlId = useId();
  const tokenId = useId();
  const [url, setUrl] = useState(credentials?.url ?? "http://127.0.0.1:");
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!validUrl(url)) return setError("Enter the server address, like http://127.0.0.1:8787.");
    if (token.trim() === "") return setError("Enter the token (the file named by `orch serve`).");
    setError(null);
    connect({ url, token });
    setToken("");
  };

  const chooseFolder = async () => {
    if (!shell) return;
    setBusy(true);
    setError(null);
    try {
      const folder = await shell.chooseProjectFolder();
      if (folder && multi) await openFolder(folder);
      else if (folder) connect(await shell.startServer(folder));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const form = (
    <form className="form" onSubmit={submit} noValidate>
      <div className="field">
        <label htmlFor={urlId}>Server address</label>
        <input
          id={urlId}
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          inputMode="url"
          autoComplete="off"
          spellCheck={false}
        />
      </div>
      <div className="field">
        <label htmlFor={tokenId}>Token</label>
        <input
          id={tokenId}
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          autoComplete="off"
          spellCheck={false}
        />
      </div>
      <p className="muted">
        Start the server with <code>orch serve</code> in the project; the token is in the file it
        names (<code>~/.config/herdr-orchestra/api-token</code>). Kept in memory only.
      </p>
      <button type="submit" className="button button--primary">
        Connect
      </button>
    </form>
  );

  return (
    <section className="panel connect" aria-labelledby="connect-title">
      <h2 id="connect-title" className="panel-title">
        Connect to a project
      </h2>
      {shell && (
        <p>
          <button type="button" className="button button--primary" onClick={chooseFolder} disabled={busy}>
            {busy ? "Starting…" : "Choose project folder…"}
          </button>
        </p>
      )}
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      {shell ? (
        <details>
          <summary>Connect to a server that is already running</summary>
          {form}
        </details>
      ) : (
        form
      )}
    </section>
  );
}
