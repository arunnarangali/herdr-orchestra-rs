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

  it("are defined for both themes, with the same variables", () => {
    const css = readFileSync(THEME, "utf8");
    const names = (block: string) => [...block.matchAll(/(--[\w-]+):/g)].map((m) => m[1] ?? "").sort();
    const light = /:root \{([^}]*)\}/.exec(css)?.[1] ?? "";
    const dark = /:root\[data-theme="dark"\] \{([^}]*)\}/.exec(css)?.[1] ?? "";
    const system = /prefers-color-scheme: dark\) \{\s*:root:not\(\[data-theme="light"\]\) \{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(names(light).length).toBeGreaterThan(10);
    expect(names(dark)).toEqual(names(light).filter((n) => !["--radius"].includes(n)));
    expect(names(system)).toEqual(names(dark));
  });

  it("gives the focus ring its own colour in both themes", () => {
    const css = readFileSync(THEME, "utf8");
    expect(css.match(/--focus:/g)?.length).toBe(3);
    const base = readFileSync(join(SRC, "styles", "base.css"), "utf8");
    expect(base).toMatch(/:focus-visible\s*\{[^}]*outline:[^;]*var\(--focus\)/);
  });
});
