import { readFileSync, readdirSync, statSync } from "node:fs";
import { extname, join, resolve } from "node:path";

const args = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  args.set(process.argv[index], process.argv[index + 1]);
}

const sourceRoot = resolve(args.get("--src") ?? "src");
const cssPath = resolve(args.get("--css") ?? join(sourceRoot, "index.css"));
const css = readFileSync(cssPath, "utf8");
const declared = new Set(
  [...css.matchAll(/--color-([a-z0-9-]+)\s*:/g)].map((match) => match[1]),
);
const semanticRoots = new Set([
  "accent",
  "background",
  "border",
  "danger",
  "destructive",
  "focus",
  "muted",
  "popover",
  "primary",
  "secondary",
  "success",
  "surface",
  "text",
  "warning",
]);
const extensions = new Set([".ts", ".tsx", ".js", ".jsx", ".html"]);
const failures = [];

function walk(directory) {
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      walk(path);
      continue;
    }
    if (!extensions.has(extname(path))) continue;
    const content = readFileSync(path, "utf8");
    const pattern =
      /(?:^|[\s"'`])(?:[a-z-]+:)*(?:bg|text|border|ring|outline)-([a-z][a-z0-9-]*)/gm;
    for (const match of content.matchAll(pattern)) {
      const token = match[1];
      const normalized = token.startsWith("offset-")
        ? token.slice("offset-".length)
        : token;
      const root = normalized.split("-")[0];
      if (semanticRoots.has(root) && !declared.has(normalized)) {
        const line = content.slice(0, match.index).split("\n").length;
        failures.push(`${path}:${line}: --color-${normalized}`);
      }
    }
  }
}

walk(sourceRoot);

if (failures.length > 0) {
  console.error(`Unknown semantic color classes:\n${failures.join("\n")}`);
  process.exit(1);
}

console.log(`Semantic color classes match ${cssPath}`);
