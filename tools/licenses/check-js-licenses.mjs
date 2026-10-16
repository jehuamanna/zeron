// Fails when a production JavaScript dependency's license is not on Zeron's
// allow-list. Mirrors the Rust policy in deny.toml.
import { execFileSync } from "node:child_process";

const ALLOWED = new Set([
  "Apache-2.0", "MIT", "MIT-0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "CC0-1.0", "0BSD", "Unlicense", "BlueOak-1.0.0",
]);

/** Packages whose metadata names no license, reviewed by hand. */
const REVIEWED = new Map([
  // The package ships an MIT license file but no `license` field.
  ["khroma", "MIT"],
]);

function allowed(expression) {
  const text = expression.replace(/[()]/g, " ").trim();
  if (text.includes(" OR ")) return text.split(" OR ").some((part) => allowed(part));
  if (text.includes(" AND ")) return text.split(" AND ").every((part) => allowed(part));
  return ALLOWED.has(text.trim());
}

const report = JSON.parse(execFileSync("pnpm", ["licenses", "list", "--prod", "--json"], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }));
const problems = [];
let count = 0;
for (const [license, packages] of Object.entries(report)) {
  for (const entry of packages) {
    count += 1;
    const effective = license === "Unknown" ? REVIEWED.get(entry.name) : license;
    if (!effective || !allowed(effective)) problems.push(`${entry.name}@${entry.versions.join(",")}: ${license}`);
  }
}
if (problems.length) {
  console.error(`Licenses not allowed:\n${problems.join("\n")}`);
  process.exit(1);
}
console.log(`All ${count} production JavaScript packages have allowed licenses.`);
