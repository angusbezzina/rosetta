import type { MountOptions } from "pele";

export {
  type LinkInfo,
  type Mounted,
  mount,
  parse,
  type RenderResult,
  render,
} from "pele";

/** The renderer's version, pinned exactly in package.json. */
export const PELE_VERSION = "1.8.0";

/**
 * How the viewer renders every source. Pele refuses a source over 50,000
 * characters by default; 200,000 bounds what one render may cost while
 * leaving room for the widest picture a repository's model asks for (a
 * Connections source caps its neighbours, so the curated views are the large
 * ones). Pele's 4 MB output cap stays.
 *
 * Pele's direction turn is off. Mounted, Pele passes the container's width
 * as maxWidth and turns a left-to-right chart top-to-bottom whenever it is
 * wider than that and the container is under directionBreakpoint, so a flow
 * in a narrow pane ran down the page. The emitter chooses each view's
 * direction and the reader pans it, so a chart is drawn as written.
 *
 * Pele's responsive style is off: it caps the SVG at its container's width
 * (max-width: 100%), which shrank a wide flow's text past reading. The SVG
 * is drawn at its own size, one layout pixel to one CSS pixel, and the
 * viewer scales it.
 *
 * Pele's zoom is off: it never zooms out past the scale it drew at, pans
 * only once zoomed in, and zooms on the wheel only with ctrl. The viewer
 * pans and zooms the diagram itself (viewer.ts, "pan and zoom").
 */
export const RENDER_OPTIONS = {
  linkSchemes: ["atlas"],
  limit: 200_000,
  autoDirection: false,
  responsive: false,
  zoom: false,
} satisfies MountOptions;
