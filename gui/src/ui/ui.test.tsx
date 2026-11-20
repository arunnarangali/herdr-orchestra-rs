import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createApiClient } from "../api/client";
import type { TasksResponse } from "../api/types";
import { TASK_STATES } from "../api/states";
import { samples } from "../api/contract";
import { errorBody, startFakeServer, type FakeServer } from "../test/fakeServer";
import { Providers } from "../test/render";
import { AttentionCard } from "../components/AttentionCard";
import { Badge } from "./Badge";
import { Button } from "./Button";
import { Card, CardHead } from "./Card";
import { ConfirmDialog } from "./ConfirmDialog";
import { Drawer } from "./Drawer";
import { Segmented } from "./Segmented";
import { Table } from "./Table";
import { ToastProvider, useToast } from "./Toast";
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

describe("shared primitives", () => {
  it("Button: tones, solid, and an unavailable action is disabled with its reason", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(
      <>
        <Button tone="emerald" solid onClick={onClick}>
          Go
        </Button>
        <Button tone="coral">Stop</Button>
        <Button unavailable="Not available yet" onClick={onClick}>
          Export
        </Button>
      </>,
    );
    expect(screen.getByRole("button", { name: "Go" })).toHaveClass("btn", "btn--emerald", "btn--solid");
    expect(screen.getByRole("button", { name: "Stop" })).toHaveClass("btn--coral");
    const off = screen.getByRole("button", { name: /Export/ });
    expect(off).toBeDisabled();
    expect(off).toHaveAttribute("title", "Not available yet");
    await user.click(off);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("Badge, Card and Table render their tone and their words", () => {
    render(
      <>
        <Badge tone="violet" mono>
          M1
        </Badge>
        <Card tone="amber" data-testid="card">
          <CardHead title="Held" />
        </Card>
        <Table caption="Milestones">
          <tbody>
            <tr>
              <td>M1</td>
            </tr>
          </tbody>
        </Table>
      </>,
    );
    expect(screen.getByText("M1", { selector: ".badge" })).toHaveClass("badge--violet", "badge--mono");
    expect(screen.getByTestId("card")).toHaveClass("card", "card--tone", "tone-amber");
    expect(screen.getByRole("heading", { name: "Held" })).toBeInTheDocument();
    expect(screen.getByRole("table", { name: "Milestones" })).toBeInTheDocument();
  });

  it("Segmented marks the chosen option and reports changes", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <Segmented
        label="View"
        value="list"
        onChange={onChange}
        options={[
          { value: "list", label: "List" },
          { value: "graph", label: "Graph" },
        ]}
      />,
    );
    expect(screen.getByRole("button", { name: "List" })).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Graph" }));
    expect(onChange).toHaveBeenCalledWith("graph");
  });

  it("Toast announces a message in a live region and removes it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    function Fire() {
      const toast = useToast();
      return <button onClick={() => toast("Saved", { tone: "emerald" })}>fire</button>;
    }
    render(
      <ToastProvider>
        <Fire />
      </ToastProvider>,
    );
    act(() => screen.getByText("fire").click());
    expect(within(screen.getByRole("status")).getByText("Saved")).toBeInTheDocument();
    act(() => void vi.advanceTimersByTime(4000));
    expect(screen.queryByText("Saved")).not.toBeInTheDocument();
    vi.useRealTimers();
  });

  it("Drawer closes on Escape and gives focus back", async () => {
    const user = userEvent.setup();
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)}>open</button>
          <Drawer open={open} title="Notifications" onClose={() => setOpen(false)}>
            <p>inside</p>
          </Drawer>
        </>
      );
    }
    render(<Harness />);
    const opener = screen.getByRole("button", { name: "open" });
    await user.click(opener);
    expect(screen.getByRole("dialog", { name: "Notifications" })).toHaveTextContent("inside");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it("AttentionCard shows the item and hands it to onReview", async () => {
    const user = userEvent.setup();
    const onReview = vi.fn();
    const it = { id: "x", kind: "failed", task: "M3", agent: "coder-1", title: "M3 failed", detail: "check failed", prompt: null, since: "", actions: [], uid: "u", sourceKey: "", projectName: "alpha" };
    render(
      <ul>
        <AttentionCard item={it} onReview={onReview} />
      </ul>,
    );
    expect(screen.getByRole("heading", { name: "M3 failed" })).toBeInTheDocument();
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.getByRole("listitem")).toHaveClass("tone-coral");
    await user.click(screen.getByRole("button", { name: "Review M3 failed" }));
    expect(onReview).toHaveBeenCalledWith(it);
  });
});
