import { useEffect, useState } from "react";
import type { ThemedToken } from "shiki/core";

import { highlight } from "./highlight";

/** A code block, plain at first and coloured once its grammar has loaded. */
export function CodeBlock({ code, language }: { code: string; language: string | undefined }) {
  const [lines, setLines] = useState<ThemedToken[][] | null>(null);
  useEffect(() => {
    let current = true;
    highlight(code, language)
      .then((tokens) => current && setLines(tokens))
      .catch(() => current && setLines(null));
    return () => {
      current = false;
    };
  }, [code, language]);
  return (
    <pre data-language={language ?? "text"} className="my-4 overflow-x-auto rounded-xl border border-white/10 bg-[#0d1117] p-4 text-[13px] leading-5">
      <code>
        {lines
          ? lines.map((line, index) => (
              <span key={index} className="block min-h-5">
                {line.map((token, at) => (
                  <span key={at} style={token.color ? { color: token.color } : undefined}>{token.content}</span>
                ))}
              </span>
            ))
          : code}
      </code>
    </pre>
  );
}
