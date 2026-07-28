#!/usr/bin/env node
// review_prompt.md §7: single source of truth for design tokens is
// src/theme/tokens.json (dark + light HSL triples); this script is the
// "JSON token -> CSS variable map" half of that requirement. It regenerates
// the block between the GENERATED markers in src/index.css so the CSS
// variables actually consumed by tailwind.config.js can never drift from
// the JSON tokens.json ships as its documented palette.
//
// Run via `npm run theme:generate` (also runs automatically before
// `dev`/`build` — see package.json) and commit the result; nothing reads
// tokens.json directly at runtime, so a stale generated block is a real bug,
// not just cosmetic. `npm run theme:check` fails CI if the two drift apart.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const tokensPath = path.join(root, "src/theme/tokens.json");
const cssPath = path.join(root, "src/index.css");

const START = "/* THEME-TOKENS:GENERATED-START (do not hand-edit — run `npm run theme:generate`) */";
const END = "/* THEME-TOKENS:GENERATED-END */";

/** Flattens one theme's token tree into `--css-var-name: value;` lines.
 * `color` and `default` are structural wrappers, not naming components:
 * color.background.default.value -> --background
 * color.card.foreground.value    -> --card-foreground
 * radius.default.value           -> --radius
 */
function flatten(node, path, out) {
  if (node && typeof node === "object" && typeof node.value === "string") {
    out.push(`    --${path.join("-")}: ${node.value};`);
    return;
  }
  for (const [key, child] of Object.entries(node)) {
    const nextPath = key === "color" || key === "default" ? path : [...path, key];
    flatten(child, nextPath, out);
  }
}

function cssBlock(themeTokens) {
  const lines = [];
  flatten(themeTokens, [], lines);
  return lines.sort().join("\n");
}

function main() {
  const tokens = JSON.parse(readFileSync(tokensPath, "utf-8"));
  if (!tokens.dark || !tokens.light) {
    throw new Error(`${tokensPath} must define both a "dark" and a "light" theme`);
  }

  const generated = [
    START,
    "  :root {",
    cssBlock(tokens.dark),
    "  }",
    "",
    "  /* Explicit dark selector too, so `dark:` Tailwind utilities work once",
    "     the app sets data-theme on <html> — not just the vars above. */",
    '  :root[data-theme="dark"] {',
    cssBlock(tokens.dark),
    "  }",
    "",
    '  :root[data-theme="light"] {',
    cssBlock(tokens.light),
    "  }",
    END,
  ].join("\n");

  const css = readFileSync(cssPath, "utf-8");
  const startIdx = css.indexOf(START);
  const endIdx = css.indexOf(END);
  if (startIdx === -1 || endIdx === -1) {
    throw new Error(
      `Could not find ${START} / ${END} markers in ${cssPath} — restore them before regenerating.`
    );
  }
  const next = css.slice(0, startIdx) + generated + css.slice(endIdx + END.length);

  if (next === css) {
    console.log("theme:generate — src/index.css already up to date with tokens.json");
    return;
  }

  if (process.argv.includes("--check")) {
    console.error(
      "theme:check — src/index.css is out of date with src/theme/tokens.json. Run `npm run theme:generate` and commit the result."
    );
    process.exit(1);
  }

  writeFileSync(cssPath, next);
  console.log("theme:generate — src/index.css regenerated from tokens.json");
}

main();
