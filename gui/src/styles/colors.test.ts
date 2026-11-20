import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(__dirname, "..");
const THEME = join(SRC, "app", "theme.css");

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? files(p) : [p];
  });
}

// hex colours, rgb()/hsl()/oklch() and the named colours people reach for
const COLOUR = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?|oklch|oklab|hwb|color-mix)\(|:\s*(?:white|black|red|green|blue|gr[ae]y|orange|yellow)\b/;

describe("colours", () => {
  it("are defined only in app/theme.css", () => {
    const offenders = files(SRC)
      .filter((f) => /\.(css|tsx?)$/.test(f) && f !== THEME && !f.endsWith(".test.ts") && !f.endsWith(".test.tsx"))
      .filter((f) => COLOUR.test(readFileSync(f, "utf8")))
      .map((f) => relative(SRC, f));
    expect(offenders).toEqual([]);
  });

  it("use the prototype's tokens exactly: dark by default, a light override", () => {
    const css = readFileSync(THEME, "utf8");
    const tokens = (block: string) =>
      Object.fromEntries([...block.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((m) => [m[1], (m[2] ?? "").trim()]));
    const dark = tokens(/:root \{([^}]*)\}/.exec(css)?.[1] ?? "");
    const light = tokens(/\[data-theme="light"\] \{([^}]*)\}/.exec(css)?.[1] ?? "");
    expect(dark).toMatchObject({
      "--bg-canvas": "#0D0F12",
      "--bg-panel": "#14171D",
      "--bg-hover": "#1C212B",
      "--border-subtle": "#262B36",
      "--text-primary": "#E6EDF3",
      "--text-muted": "#8B949E",
      "--accent-emerald": "#3FB950",
      "--accent-cyan": "#58A6FF",
      "--accent-amber": "#D29922",
      "--accent-coral": "#F85149",
      "--accent-violet": "#BC8CFF",
      "--on-solid": "#04130A",
      "--backdrop": "rgb(0 0 0 / 0.55)",
    });
    expect(light).toMatchObject({
      "--bg-canvas": "#F6F8FA",
      "--bg-panel": "#FFFFFF",
      "--bg-hover": "#EAEEF2",
      "--border-subtle": "#D0D7DE",
      "--text-primary": "#1F2328",
      "--text-muted": "#59636E",
      "--accent-emerald": "#1A7F37",
      "--accent-cyan": "#0969DA",
      "--accent-amber": "#9A6700",
      "--accent-coral": "#CF222E",
      "--accent-violet": "#8250DF",
      "--on-solid": "#FFFFFF",
      "--backdrop": "rgb(31 35 40 / 0.35)",
    });
    // the light block overrides exactly the tokens the dark block defines
    expect(Object.keys(light).sort()).toEqual(Object.keys(dark).sort());
  });

  it("gives every control one focus ring in the accent colour", () => {
    const base = readFileSync(join(SRC, "styles", "base.css"), "utf8");
    expect(base).toMatch(/:focus-visible\s*\{[^}]*outline:[^;]*var\(--accent-cyan\)/);
  });
});
