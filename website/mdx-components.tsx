import type { MDXComponents } from 'mdx/types';

export function useMDXComponents(components: MDXComponents): MDXComponents {
  return {
    h1: ({ children }) => (
      <h1 className="text-3xl font-bold mt-8 mb-4">{children}</h1>
    ),
    h2: ({ children }) => (
      <h2 className="text-2xl font-semibold mt-6 mb-3">{children}</h2>
    ),
    h3: ({ children }) => (
      <h3 className="text-xl font-semibold mt-4 mb-2">{children}</h3>
    ),
    p: ({ children }) => (
      <p className="my-3 leading-relaxed">{children}</p>
    ),
    code: ({ children, ...props }) => (
      <code
        className="bg-[var(--color-code-bg)] px-1.5 py-0.5 rounded text-sm font-mono"
        {...props}
      >
        {children}
      </code>
    ),
    pre: ({ children, ...props }) => (
      <pre
        className="bg-[var(--color-code-bg)] p-4 rounded-lg overflow-x-auto my-4 font-mono text-sm"
        {...props}
      >
        {children}
      </pre>
    ),
    ul: ({ children }) => (
      <ul className="list-disc list-inside my-3 space-y-1">{children}</ul>
    ),
    ol: ({ children }) => (
      <ol className="list-decimal list-inside my-3 space-y-1">{children}</ol>
    ),
    a: ({ children, href }) => (
      <a href={href} className="text-[var(--color-accent)] hover:underline">
        {children}
      </a>
    ),
    ...components,
  };
}
