interface CodeBlockProps {
  code: string;
  language?: string;
  filename?: string;
}

export function CodeBlock({ code, language, filename }: CodeBlockProps) {
  return (
    <div className="rounded-lg overflow-hidden border border-[var(--color-border)]">
      {filename && (
        <div className="bg-[var(--color-bg-secondary)] px-4 py-2 text-xs font-mono text-[var(--color-text-secondary)] border-b border-[var(--color-border)]">
          {filename}
        </div>
      )}
      <pre className="bg-[var(--color-code-bg)] p-4 overflow-x-auto text-sm leading-relaxed">
        <code className={language ? `language-${language}` : undefined}>
          {code}
        </code>
      </pre>
    </div>
  );
}
