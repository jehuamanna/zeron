import type { HighlighterCore, ThemedToken } from "shiki/core";

/** Grammars bundled with the reader, each loaded the first time it is used. */
const GRAMMARS: Record<string, () => Promise<unknown>> = {
  bash: () => import("shiki/langs/bash.mjs"),
  c: () => import("shiki/langs/c.mjs"),
  cpp: () => import("shiki/langs/cpp.mjs"),
  csharp: () => import("shiki/langs/csharp.mjs"),
  css: () => import("shiki/langs/css.mjs"),
  go: () => import("shiki/langs/go.mjs"),
  html: () => import("shiki/langs/html.mjs"),
  java: () => import("shiki/langs/java.mjs"),
  javascript: () => import("shiki/langs/javascript.mjs"),
  json: () => import("shiki/langs/json.mjs"),
  jsx: () => import("shiki/langs/jsx.mjs"),
  kotlin: () => import("shiki/langs/kotlin.mjs"),
  markdown: () => import("shiki/langs/markdown.mjs"),
  php: () => import("shiki/langs/php.mjs"),
  python: () => import("shiki/langs/python.mjs"),
  ruby: () => import("shiki/langs/ruby.mjs"),
  rust: () => import("shiki/langs/rust.mjs"),
  sql: () => import("shiki/langs/sql.mjs"),
  swift: () => import("shiki/langs/swift.mjs"),
  toml: () => import("shiki/langs/toml.mjs"),
  tsx: () => import("shiki/langs/tsx.mjs"),
  typescript: () => import("shiki/langs/typescript.mjs"),
  yaml: () => import("shiki/langs/yaml.mjs"),
};

const ALIASES: Record<string, string> = {
  "c++": "cpp", "c#": "csharp", cs: "csharp", golang: "go", js: "javascript", kt: "kotlin", md: "markdown",
  py: "python", rb: "ruby", rs: "rust", sh: "bash", shell: "bash", ts: "typescript", yml: "yaml", zsh: "bash",
};

/** The bundled grammar for a fence's language, or `null`. */
export function grammarFor(language: string | undefined): string | null {
  if (!language) return null;
  const name = language.toLowerCase();
  const resolved = ALIASES[name] ?? name;
  return resolved in GRAMMARS ? resolved : null;
}

let highlighter: Promise<HighlighterCore> | null = null;
const loaded = new Map<string, Promise<void>>();

function core(): Promise<HighlighterCore> {
  // The JavaScript regex engine: the CSP allows no WebAssembly.
  highlighter ??= Promise.all([import("shiki/core"), import("shiki/engine/javascript")]).then(([{ createHighlighterCore }, { createJavaScriptRegexEngine }]) =>
    createHighlighterCore({ themes: [import("shiki/themes/github-dark.mjs")], langs: [], engine: createJavaScriptRegexEngine() }),
  );
  return highlighter;
}

/** Coloured tokens for `code`, or `null` when its language is not bundled. */
export async function highlight(code: string, language: string | undefined): Promise<ThemedToken[][] | null> {
  const grammar = grammarFor(language);
  if (!grammar) return null;
  const shiki = await core();
  let load = loaded.get(grammar);
  if (!load) {
    load = GRAMMARS[grammar]!().then((module) => shiki.loadLanguage((module as { default: Parameters<HighlighterCore["loadLanguage"]>[0] }).default));
    loaded.set(grammar, load);
  }
  await load;
  return shiki.codeToTokensBase(code, { lang: grammar, theme: "github-dark" });
}
