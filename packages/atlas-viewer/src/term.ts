import type { AtlasTerm } from "./types.ts";

/** The class the glossary block carries, so its styles need no atlas around it. */
export const GLOSSARY_CLASS = "virgil-atlas-glossary";

/**
 * A glossary term as the card over a word in a message draws it: the name
 * (marked Proposed while it is), the definition and the Avoid line.
 * Framework-free, so a host mounts it with the stylesheet alone.
 */
export function renderTerm(doc: Document, term: AtlasTerm): HTMLElement {
  const el = (tag: string, className?: string, text?: string) => {
    const node = doc.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const root = el("div", GLOSSARY_CLASS);
  const row = el("div", "virgil-atlas-term-row");
  row.append(el("span", "virgil-atlas-term", term.name));
  if (term.status === "proposed") {
    row.append(el("span", "virgil-atlas-proposed", "Proposed"));
  }
  const definition = el("p", "virgil-atlas-def");
  definition.append(...inlineCode(doc, term.definition));
  const avoid = el("p", "virgil-atlas-avoid");
  const words =
    term.avoid.length === 0
      ? "none"
      : term.avoid
          .map((a) =>
            a.except.length > 0
              ? `${a.word} (except ${a.except.join(", ")})`
              : a.word,
          )
          .join(", ");
  avoid.append(el("b", undefined, "Avoid"), ` ${words}`);
  root.append(row, definition, avoid);
  return root;
}

/** Definitions mark code with backticks, as GLOSSARY.md renders them. */
function inlineCode(doc: Document, text: string): Node[] {
  return text.split("`").map((part, i) => {
    if (i % 2 === 0) return doc.createTextNode(part);
    const code = doc.createElement("code");
    code.textContent = part;
    return code;
  });
}
