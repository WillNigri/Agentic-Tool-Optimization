import { useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";
import { Copy, Check } from "lucide-react";
import "highlight.js/styles/github-dark.css";

// v1.5.0 — Render assistant messages as markdown.
//
// react-markdown + remark-gfm (tables, strikethrough, task lists) +
// rehype-highlight (syntax highlighting via highlight.js).
// Code blocks get a Copy button. Headings/lists/links use Tailwind via the
// `prose-*` utility-style overrides we apply per element to stay on-theme
// (cs-text / cs-muted / cs-accent) without pulling in @tailwindcss/typography.

interface Props {
  content: string;
}

export default function MarkdownContent({ content }: Props) {
  const components = useMemo(() => buildComponents(), []);
  return (
    <div className="markdown-body text-xs leading-relaxed text-cs-text">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
        components={components}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

function buildComponents() {
  return {
    h1: (props: React.ComponentProps<"h1">) => (
      <h1 className="text-base font-semibold text-cs-text mt-3 mb-1.5" {...props} />
    ),
    h2: (props: React.ComponentProps<"h2">) => (
      <h2 className="text-sm font-semibold text-cs-text mt-3 mb-1.5" {...props} />
    ),
    h3: (props: React.ComponentProps<"h3">) => (
      <h3 className="text-xs font-semibold text-cs-text uppercase tracking-wide mt-2 mb-1" {...props} />
    ),
    p: (props: React.ComponentProps<"p">) => (
      <p className="text-xs text-cs-text my-1.5" {...props} />
    ),
    ul: (props: React.ComponentProps<"ul">) => (
      <ul className="list-disc pl-5 my-1.5 space-y-0.5 text-xs" {...props} />
    ),
    ol: (props: React.ComponentProps<"ol">) => (
      <ol className="list-decimal pl-5 my-1.5 space-y-0.5 text-xs" {...props} />
    ),
    li: (props: React.ComponentProps<"li">) => (
      <li className="text-cs-text" {...props} />
    ),
    a: (props: React.ComponentProps<"a">) => (
      <a className="text-cs-accent underline underline-offset-2 hover:no-underline" target="_blank" rel="noreferrer noopener" {...props} />
    ),
    strong: (props: React.ComponentProps<"strong">) => (
      <strong className="font-semibold text-cs-text" {...props} />
    ),
    em: (props: React.ComponentProps<"em">) => (
      <em className="italic text-cs-text" {...props} />
    ),
    blockquote: (props: React.ComponentProps<"blockquote">) => (
      <blockquote className="border-l-2 border-cs-accent/40 pl-3 my-2 text-cs-muted italic" {...props} />
    ),
    hr: () => <hr className="border-cs-border my-3" />,
    table: (props: React.ComponentProps<"table">) => (
      <div className="overflow-x-auto my-2">
        <table className="text-xs border-collapse" {...props} />
      </div>
    ),
    th: (props: React.ComponentProps<"th">) => (
      <th className="border border-cs-border px-2 py-1 text-left font-semibold text-cs-text bg-cs-bg-raised" {...props} />
    ),
    td: (props: React.ComponentProps<"td">) => (
      <td className="border border-cs-border px-2 py-1 text-cs-text" {...props} />
    ),
    code: ({ className, children, ...rest }: React.ComponentProps<"code"> & { inline?: boolean }) => {
      const isInline = !className?.includes("language-");
      if (isInline) {
        return (
          <code
            className="rounded bg-cs-bg-raised border border-cs-border px-1 py-0.5 text-[11px] font-mono text-cs-accent"
            {...rest}
          >
            {children}
          </code>
        );
      }
      return (
        <code className={`${className} text-[11px] font-mono`} {...rest}>
          {children}
        </code>
      );
    },
    pre: ({ children }: React.ComponentProps<"pre">) => {
      // Extract the raw text from the nested <code> child for copying.
      // react-markdown wraps code blocks as <pre><code class="language-x">...
      const codeText = extractText(children);
      return <CodeBlock raw={codeText}>{children}</CodeBlock>;
    },
  };
}

function extractText(node: React.ReactNode): string {
  if (typeof node === "string") return node;
  if (typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(extractText).join("");
  // React element — peek at props.children.
  if (
    node &&
    typeof node === "object" &&
    "props" in node &&
    (node as { props?: { children?: React.ReactNode } }).props?.children !== undefined
  ) {
    return extractText((node as { props: { children: React.ReactNode } }).props.children);
  }
  return "";
}

function CodeBlock({ children, raw }: { children: React.ReactNode; raw: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = () => {
    void navigator.clipboard.writeText(raw);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="relative group my-2">
      <pre className="rounded-md border border-cs-border bg-cs-bg p-2.5 overflow-x-auto text-[11px] leading-relaxed">
        {children}
      </pre>
      <button
        type="button"
        onClick={handleCopy}
        className="absolute top-1.5 right-1.5 opacity-0 group-hover:opacity-100 transition-opacity inline-flex items-center gap-1 rounded border border-cs-border bg-cs-bg-raised px-1.5 py-0.5 text-[9px] text-cs-muted hover:text-cs-accent"
      >
        {copied ? <Check size={9} /> : <Copy size={9} />}
        {copied ? "copied" : "copy"}
      </button>
    </div>
  );
}
