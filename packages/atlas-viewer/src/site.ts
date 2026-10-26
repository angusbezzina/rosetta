/** The static export uses a classic script so it also runs from file://. */
import { boot } from "./standalone.ts";

const root = document.getElementById("atlas");
if (root) void boot(root);
