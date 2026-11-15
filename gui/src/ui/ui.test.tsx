import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createApiClient } from "../api/client";
import type { TasksResponse } from "../api/types";
import { TASK_STATES } from "../api/states";
import { samples } from "../api/contract";
import { errorBody, startFakeServer, type FakeServer } from "../test/fakeServer";
import { Providers } from "../test/render";
import { ConfirmDialog } from "./ConfirmDialog";
import { ResourceView } from "./ResourceView";
import { StatusBadge, toneOf } from "./StatusBadge";
import { useResource } from "./useResource";

describe("StatusBadge", () => {
  it("says the state in words and has a tone for every state", () => {
    for (const state of TASK_STATES) {
      const { unmount } = render(<StatusBadge state={state} />);
      expect(screen.getByText(state)).toHaveAttribute("data-state", state);
      unmount();
    }
    expect(toneOf("merged")).toBe("success");
    expect(toneOf("failed")).toBe("danger");
    expect(toneOf("needs-answer")).toBe("warning");
    expect(toneOf("coding")).toBe("info");
    expect(toneOf("something-new")).toBe("neutral");
  });
});

describe("ConfirmDialog", () => {
  function Harness({ onConfirm }: { onConfirm: () => void }) {
    const [open, setOpen] = useState(false);
    return (
      <>
        <button onClick={() => setOpen(true)}>Stop run</button>
        <ConfirmDialog
          open={open}
          title="Stop the run?"
          confirmLabel="Stop"
          destructive
          onConfirm={() => {
            setOpen(false);
            onConfirm();
          }}
          onCancel={() => setOpen(false)}
        >
          Running coders save their work first.
        </ConfirmDialog>
      </>
    );
  }

  it("asks first, with focus on Cancel, and returns focus when closed", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<Harness onConfirm={onConfirm} />);
    const opener = screen.getByRole("button", { name: "Stop run" });
    await user.click(opener);
    const dialog = screen.getByRole("alertdialog", { name: "Stop the run?" });
    expect(dialog).toHaveTextContent("save their work");
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onConfirm).not.toHaveBeenCalled();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it("confirms only on the confirm button; Escape cancels; Tab stays inside", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<Harness onConfirm={onConfirm} />);
    await user.click(screen.getByRole("button", { name: "Stop run" }));
    await user.keyboard("{Escape}");
    expect(onConfirm).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Stop run" }));
    await user.tab();
    expect(screen.getByRole("button", { name: "Stop" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.tab({ shift: true });
    expect(screen.getByRole("button", { name: "Stop" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onConfirm).toHaveBeenCalledOnce();
  });
});

describe("useResource and ResourceView", () => {
  let server: FakeServer;
  beforeEach(async () => {
    server = await startFakeServer();
  });
  afterEach(() => server.close());

  function Tasks({ onApi }: { onApi?: (c: ReturnType<typeof createApiClient>) => void }) {
    const r = useResource((api, signal) => {
      onApi?.(api);
      return api.tasks({ signal });
    }, [], { isEmpty: (d: TasksResponse) => d.tasks.length === 0, refreshOn: ["task-state"], refreshDelayMs: 20 });
    return (
      <ResourceView resource={r} empty="No milestones yet.">
        {(d) => <ul>{d.tasks.map((t) => <li key={t.id}>{t.title}</li>)}</ul>}
      </ResourceView>
    );
  }
  const show = () =>
    render(
      <Providers initial={server}>
        <Tasks />
      </Providers>,
    );

  it("loading, then the data", async () => {
    server.respond("GET", "/tasks", 200, samples.tasks);
    show();
    expect(screen.getByRole("status")).toHaveTextContent("Loading");
    expect(await screen.findByText("Foundation")).toBeInTheDocument();
  });

  it("empty", async () => {
    server.respond("GET", "/tasks", 200, { tasks: [] });
    show();
    expect(await screen.findByText("No milestones yet.")).toBeInTheDocument();
  });

  it("error with what to do, and a retry that works", async () => {
    const user = userEvent.setup();
    server.respond("GET", "/tasks", 404, errorBody("no_run", "no run exists"));
    show();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("This project has no run yet");
    expect(alert).toHaveTextContent("orch sync");

    server.respond("GET", "/tasks", 200, samples.tasks);
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("Foundation")).toBeInTheDocument();
  });

  it("an expired token reads as such", async () => {
    server.respond("GET", "/tasks", 401, errorBody("unauthorized", "wrong token"));
    show();
    expect(await screen.findByText("The server refused the token")).toBeInTheDocument();
  });

  it("reloads when the live stream delivers an event it listens for", async () => {
    server.respond("GET", "/tasks", 200, samples.tasks);
    show();
    await screen.findByText("Foundation");
    await waitFor(() => expect(server.streamRequests()).toHaveLength(1));
    await new Promise((r) => setTimeout(r, 50));
    const before = server.requests.filter((r) => r.path === "/tasks").length;
    server.respond("GET", "/tasks", 200, { tasks: [{ ...samples.tasks.tasks[0], title: "Renamed" }] });
    server.emit(1, "log", { message: "x", line: "x", ts: 1 }); // not listened for
    server.emit(2, "task-state", { id: "M1" });
    expect(await screen.findByText("Renamed")).toBeInTheDocument();
    expect(server.requests.filter((r) => r.path === "/tasks").length).toBe(before + 1);
  });
});
