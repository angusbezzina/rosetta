// The demo page: the fixture atlas with a theme toggle and a brand swap. The
// query string sets the starting state so a headless browser can photograph
// each one: ?theme=dark&view=connections&root=door&select=door&find=serve
import {
  type AtlasBrand,
  type AtlasTheme,
  mount,
  readBundle,
} from "../src/index.ts";
import { mountPanel } from "../src/panel.ts";
import fixture from "./fixture-bundle.json" with { type: "json" };

const BRANDS: Record<string, Partial<AtlasBrand> | undefined> = {
  neutral: undefined,
  forest: { primary: "#2F8F5B", secondary: "#8C6BD9", tertiary: "#D9534F" },
};

const params = new URLSearchParams(location.search);
const root = document.documentElement;
const themeButton = document.getElementById("theme") as HTMLButtonElement;
const brandSelect = document.getElementById("brand") as HTMLSelectElement;

let theme: AtlasTheme = params.get("theme") === "dark" ? "dark" : "light";
const brandName = params.get("brand") ?? "neutral";
brandSelect.value = brandName;

const atlas = mount(document.getElementById("atlas") as HTMLElement, {
  bundle: readBundle(fixture),
  ...(BRANDS[brandName] === undefined ? {} : { brand: BRANDS[brandName] }),
  theme,
});
mountPanel(atlas);

const applyTheme = () => {
  root.dataset.theme = theme;
  root.classList.toggle("dark", theme === "dark");
  themeButton.textContent = theme === "dark" ? "Light" : "Dark";
  atlas.setTheme(theme);
};
applyTheme();

themeButton.addEventListener("click", () => {
  theme = theme === "dark" ? "light" : "dark";
  applyTheme();
});
brandSelect.addEventListener("change", () => {
  atlas.setBrand(BRANDS[brandSelect.value]);
});

const view = params.get("view");
if (view) atlas.open(view, params.get("root") ?? undefined);
const select = params.get("select");
if (select) atlas.select(select);
const find = params.get("find");
if (find !== null) atlas.find(find);
