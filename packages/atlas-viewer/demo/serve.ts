// `bun run demo`: serves the demo page with Bun's HTML bundler, and Pele from
// the vendored copy at ./pele.js for a look at dist/index.html alongside.
import { join } from "node:path";
import page from "./index.html";

const PKG = join(import.meta.dir, "..");
const port = Number(process.env.PORT ?? 4317);

const server = Bun.serve({
  port,
  development: true,
  routes: {
    "/": page,
    "/dist/": () => new Response(Bun.file(join(PKG, "dist/index.html"))),
    "/dist/pele.js": () =>
      new Response(Bun.file(join(PKG, "dist/pele.js"))),
    "/dist/bundle.json": () =>
      new Response(Bun.file(join(PKG, "demo/fixture-bundle.json"))),
    "/dist/:file": (req) =>
      new Response(Bun.file(join(PKG, "dist", req.params.file))),
  },
});

console.log(
  `atlas viewer demo: ${server.url}  (dist page: ${server.url}dist/)`,
);
