import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

/** `system` is offered in Settings; the top bar toggles between `light` and `dark`. */
export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "orch.theme";
export const THEME_CHOICES: readonly { value: ThemePreference; label: string }[] = [
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
  { value: "system", label: "System" },
];

/** Dark is the default; a saved choice wins. */
function readPreference(): ThemePreference {
  try {
    const v = window.localStorage.getItem(STORAGE_KEY);
    return v === "light" || v === "dark" || v === "system" ? v : "dark";
  } catch {
    return "dark";
  }
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

function systemIsDark(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(DARK_QUERY).matches;
}

interface ThemeContext {
  preference: ThemePreference;
  /** What is shown now: the preference, or the system's setting when it is `system`. */
  resolved: ResolvedTheme;
  setPreference: (p: ThemePreference) => void;
  /** Switch between light and dark (what the top bar button does) and remember it. */
  toggle: () => void;
}

const Ctx = createContext<ThemeContext | null>(null);

/**
 * Dark by default, light as an override. The shown theme is written as `data-theme` on <html>
 * (the colours live in `theme.css`) and the choice is kept in local storage.
 */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [preference, setPreferenceState] = useState<ThemePreference>(readPreference);
  const [dark, setDark] = useState(systemIsDark);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(DARK_QUERY);
    const onChange = (e: MediaQueryListEvent) => setDark(e.matches);
    setDark(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const resolved: ResolvedTheme = preference === "system" ? (dark ? "dark" : "light") : preference;

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", resolved);
  }, [resolved]);

  const value = useMemo<ThemeContext>(() => {
    const setPreference = (p: ThemePreference) => {
      setPreferenceState(p);
      try {
        window.localStorage.setItem(STORAGE_KEY, p);
      } catch {
        // storage is optional: the choice then lasts until the app closes
      }
    };
    return {
      preference,
      resolved,
      setPreference,
      toggle: () => setPreference(resolved === "dark" ? "light" : "dark"),
    };
  }, [preference, resolved]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useTheme(): ThemeContext {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useTheme needs a ThemeProvider");
  return ctx;
}
