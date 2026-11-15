import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "orch.theme";
export const THEME_CHOICES: readonly { value: ThemePreference; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

function readPreference(): ThemePreference {
  try {
    const v = window.localStorage.getItem(STORAGE_KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
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
}

const Ctx = createContext<ThemeContext | null>(null);

/**
 * Light and dark follow the system setting until the user picks one. The choice is only
 * written as `data-theme` on <html>; the colours live in `theme.css`.
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

  useEffect(() => {
    const root = document.documentElement;
    if (preference === "system") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", preference);
  }, [preference]);

  const value = useMemo<ThemeContext>(
    () => ({
      preference,
      resolved: preference === "system" ? (dark ? "dark" : "light") : preference,
      setPreference: (p) => {
        setPreferenceState(p);
        try {
          if (p === "system") window.localStorage.removeItem(STORAGE_KEY);
          else window.localStorage.setItem(STORAGE_KEY, p);
        } catch {
          // storage is optional: the choice then lasts until the app closes
        }
      },
    }),
    [preference, dark],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useTheme(): ThemeContext {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useTheme needs a ThemeProvider");
  return ctx;
}
