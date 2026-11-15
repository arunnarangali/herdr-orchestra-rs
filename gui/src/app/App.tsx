import { HashRouter, Navigate, Route, Routes, useLocation } from "react-router-dom";
import { useProjects } from "./projects";
import { ConnectionProvider, useConnection, type ConnectionProviderProps } from "./connection";
import { Frame } from "./Frame";
import { DEFAULT_PATH, ONBOARDING_PATH, screens, type ScreenDef } from "./routes";
import { ThemeProvider } from "./theme";

/** Until a project is chosen only Getting started is shown; every other screen redirects to it. */
function Guarded({ screen }: { screen: ScreenDef }) {
  const { kind } = useConnection();
  const { pathname } = useLocation();
  const { restoring } = useProjects();
  if (restoring) return null; // the frame says "Opening your projects…"
  if (kind === "unconfigured" && pathname !== ONBOARDING_PATH) {
    return <Navigate to={ONBOARDING_PATH} replace />;
  }
  return <screen.Component />;
}

/** The routes, from the screen registry. Needs a router, the theme and the connection around it. */
export function AppRoutes() {
  return (
    <Routes>
      <Route element={<Frame />}>
        <Route index element={<Navigate to={DEFAULT_PATH} replace />} />
        {screens.map((s) => (
          <Route key={s.id} path={s.path} element={<Guarded screen={s} />} />
        ))}
        <Route path="*" element={<Navigate to={DEFAULT_PATH} replace />} />
      </Route>
    </Routes>
  );
}

/** The whole app. A hash router so it works from any origin the shell serves it on. */
export function App(props: Omit<ConnectionProviderProps, "children">) {
  return (
    <ThemeProvider>
      <ConnectionProvider {...props}>
        <HashRouter>
          <AppRoutes />
        </HashRouter>
      </ConnectionProvider>
    </ThemeProvider>
  );
}
