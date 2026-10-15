import { useQuery } from "@tanstack/react-query";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { asCommandError, libraryApi } from "../../services/library";
import { CodeBlock } from "./CodeBlock";

const components: Components = {
  // Lesson Markdown cannot reach the web or the disk: links and images show as text.
  a: ({ children, href }) => <span className="text-cyan-200 underline decoration-dotted" title={href}>{children}</span>,
  img: ({ alt }) => <span className="text-slate-400">[image{alt ? `: ${alt}` : ""}]</span>,
  pre: ({ children }) => <>{children}</>,
  code: ({ className, children }) => {
    const language = /language-([\w#+-]+)/.exec(className ?? "")?.[1];
    const text = String(children);
    return language || text.includes("\n") ? (
      <CodeBlock code={text.replace(/\n$/, "")} language={language} />
    ) : (
      <code className="rounded bg-white/10 px-1 py-0.5 text-[0.9em]">{children}</code>
    );
  },
};

/** A Markdown or plain-text lesson resource. */
export function TextReader({ courseId, resourceId, title }: { courseId: string; resourceId: string; title: string }) {
  const resource = useQuery({
    queryKey: ["text-resource", courseId, resourceId],
    queryFn: () => libraryApi.readText(courseId, resourceId),
    staleTime: Infinity,
  });
  if (resource.isPending) return <p className="text-sm text-slate-400">Opening {title}…</p>;
  if (resource.isError) {
    return <p role="alert" className="text-sm text-red-300">{asCommandError(resource.error)?.message ?? "This resource could not be opened."}</p>;
  }
  return (
    <article aria-label={`Reading ${title}`} className="reader max-h-[70vh] overflow-y-auto pr-2 text-[15px] leading-7 text-slate-200">
      {resource.data.format === "markdown" ? (
        <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml components={components}>
          {resource.data.text}
        </ReactMarkdown>
      ) : (
        <pre className="whitespace-pre-wrap font-sans">{resource.data.text}</pre>
      )}
    </article>
  );
}
