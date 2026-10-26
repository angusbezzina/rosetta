/** The resolved brand colours, `#RRGGBB` as brand.schema.json allows. */
export interface AtlasBrand {
  primary: string;
  secondary: string;
  tertiary: string;
}

export type AtlasTheme = "light" | "dark";

/** Neutral defaults; a host can pass its own brand or set the CSS tokens. */
export const DEFAULT_BRAND: AtlasBrand = {
  primary: "#4066A6",
  secondary: "#367C70",
  tertiary: "#A96A38",
};

const COLOR = /^#[0-9a-f]{6}$/i;

/** The brand with any missing or malformed colour taken from the defaults. */
export function resolveBrand(brand?: Partial<AtlasBrand>): AtlasBrand {
  const pick = (key: keyof AtlasBrand) => {
    const value = brand?.[key];
    return value && COLOR.test(value) ? value : DEFAULT_BRAND[key];
  };
  return {
    primary: pick("primary"),
    secondary: pick("secondary"),
    tertiary: pick("tertiary"),
  };
}

/** A host's CSS token values, read before inline viewer variables are set. */
export function cssBrand(el: HTMLElement): Partial<AtlasBrand> {
  const style = el.ownerDocument.defaultView?.getComputedStyle(el);
  const read = (name: string) => style?.getPropertyValue(name).trim() || undefined;
  return {
    primary: read("--rosetta-surface"),
    secondary: read("--rosetta-service"),
    tertiary: read("--rosetta-store"),
  };
}

// A family's fill is its colour mixed into the ground; dark grounds need more
// of it to read as the same tint.
const TINT: Record<AtlasTheme, string> = { light: "12%", dark: "20%" };

/**
 * The custom properties the viewer sets on its mount element. Families take
 * the brand (surface primary, service secondary, store tertiary); agent and
 * external take the host's neutral palette, which the host already switches
 * with its theme, so a theme swap only changes the tint. A host can define
 * `--rosetta-surface`, `--rosetta-service`, `--rosetta-store`,
 * `--rosetta-agent` and `--rosetta-external` in CSS.
 */
export function brandVariables(
  brand: AtlasBrand,
  theme: AtlasTheme,
): Record<string, string> {
  return {
    "--atlas-surface": brand.primary,
    "--atlas-service": brand.secondary,
    "--atlas-store": brand.tertiary,
    "--atlas-agent": "var(--rosetta-agent, hsl(var(--muted-foreground)))",
    "--atlas-external": "var(--rosetta-external, hsl(var(--border)))",
    "--atlas-tint": TINT[theme],
    "--pele-bg": "hsl(var(--surface-content, var(--background)))",
    "--pele-fg": "hsl(var(--foreground))",
    "--pele-muted": "hsl(var(--muted-foreground))",
    "--pele-line": "hsl(var(--muted-foreground))",
    "--pele-surface": "hsl(var(--muted))",
    "--pele-surface-alt": "hsl(var(--accent))",
    "--pele-border": "hsl(var(--border))",
    "--pele-accent": brand.primary,
    "--pele-series-1": brand.primary,
    "--pele-series-2": brand.secondary,
    "--pele-series-3": brand.tertiary,
    "--pele-series-4": "hsl(var(--muted-foreground))",
    "--pele-series-5": "hsl(var(--border))",
    "--pele-font": "var(--font-sans, ui-sans-serif, system-ui, sans-serif)",
    "--pele-font-mono": "var(--font-mono, ui-monospace, monospace)",
    "--pele-radius": "var(--radius-md, 6px)",
  };
}
