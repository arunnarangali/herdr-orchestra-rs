import { render } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import type { Credentials } from "../api/client";
import { AppRoutes } from "../app/App";
import { ConnectionProvider } from "../app/connection";
import type { Shell } from "../app/shell";
import { ThemeProvider } from "../app/theme";
import type { ReactNode } from "react";

export interface RenderOptions {
  route?: string;
  initial?: Credentials;
  shell?: Shell | null;
}

/** Providers with fast retry timings, for tests of anything that talks to the server. */
export function Providers({ children, initial, shell = null, route = "/" }: RenderOptions & { children: ReactNode }) {
  return (
    <ThemeProvider>
      <ConnectionProvider initial={initial} shell={shell} retryDelaysMs={[20]} pollMs={50}>
        <MemoryRouter initialEntries={[route]}>{children}</MemoryRouter>
      </ConnectionProvider>
    </ThemeProvider>
  );
}

/** The whole app (frame and routes) at `route`. */
export function renderApp(options: RenderOptions = {}) {
  return render(
    <Providers {...options}>
      <AppRoutes />
    </Providers>,
  );
}

/** A shell double whose commands the test controls. */
export function fakeShell(overrides: Partial<Shell> = {}): Shell {
  return {
    chooseProjectFolder: async () => null,
    startServer: async () => {
      throw new Error("startServer not set up");
    },
    stopServer: async () => {},
    getConnection: async () => null,
    findOrch: async () => "/usr/bin/orch",
    findHerdr: async () => "/usr/bin/herdr",
    detectAgentClis: async () => [],
    getProjectSettings: async (path) => ({ leadAgent: null, folder: path, warning: null }),
    saveProjectSettings: async (_path, settings) => ({ leadAgent: settings.leadAgent, folder: settings.folder, warning: null }),
    ...overrides,
  };
}
