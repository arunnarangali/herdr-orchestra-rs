import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { screens } from "../../app/routes";
import { startFakeServer, type FakeServer } from "../../test/fakeServer";
import { renderApp } from "../../test/render";

let server: FakeServer;
beforeEach(async () => {
  server = await startFakeServer();
});
afterEach(() => server.close());

const show = () => renderApp({ route: "/help", initial: server });
const region = (name: RegExp) => screen.getByRole("region", { name });

describe("Help", () => {
  it("lists the help topics", async () => {
    show();
    expect(await screen.findByRole("heading", { name: "Help", level: 1 })).toBeInTheDocument();
    for (const name of [/Core CLI Commands/, /Exit Codes/, /Environment Tunables/, /Keyboard Shortcuts/, /Architecture Guarantees/]) {
      expect(region(name)).toBeInTheDocument();
    }
    expect(region(/Core CLI Commands/)).toHaveTextContent("orch approve M4");
    expect(region(/Exit Codes/)).toHaveTextContent("Merge Conflict");
    expect(region(/Environment Tunables/)).toHaveTextContent("ORCH_RISK_MAX_LINES");
  });

  it("lists every keyboard shortcut the app has", async () => {
    show();
    const keys = await screen.findByRole("table", { name: "Keyboard shortcuts" });
    for (const s of screens.filter((x) => x.shortcut)) {
      expect(within(keys).getByText(`Go to ${s.label}`)).toBeInTheDocument();
    }
    expect(keys).toHaveTextContent("Open Command Palette");
  });

  it("filters as you type, says when nothing matches, and Clear brings everything back", async () => {
    const user = userEvent.setup();
    show();
    const search = await screen.findByRole("searchbox", { name: "Search help" });
    // each word arrives as one input event, as when pasted (the "/" test types key by key)
    await user.click(search);
    await user.paste("conflict");
    expect(region(/Exit Codes/)).toHaveTextContent("Merge Conflict");
    expect(within(region(/Exit Codes/)).getAllByRole("row")).toHaveLength(2); // header + one code
    expect(screen.queryByRole("region", { name: /Keyboard Shortcuts/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: /Core CLI Commands/ })).not.toBeInTheDocument();

    await user.clear(search);
    await user.paste("approve");
    expect(region(/Core CLI Commands/)).toHaveTextContent("orch approve M4");
    expect(region(/Core CLI Commands/)).not.toHaveTextContent("orch doctor");

    await user.clear(search);
    await user.paste("zzzz");
    expect(screen.getByText(/No matches for/)).toHaveTextContent("zzzz");
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(search).toHaveValue("");
    expect(search).toHaveFocus();
    expect(region(/Keyboard Shortcuts/)).toBeInTheDocument();
  });

  it("/ focuses the search and Escape clears it", async () => {
    const user = userEvent.setup();
    show();
    const search = await screen.findByRole("searchbox", { name: "Search help" });
    await user.keyboard("/");
    expect(search).toHaveFocus();
    await user.keyboard("lease{Escape}");
    expect(search).toHaveValue("");
  });

  it("command groups fold and unfold", async () => {
    const user = userEvent.setup();
    show();
    const btn = await screen.findByRole("button", { name: /Diagnostics/ });
    expect(btn).toHaveAttribute("aria-expanded", "true");
    await user.click(btn);
    expect(btn).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("orch doctor")).not.toBeVisible();
  });
});
