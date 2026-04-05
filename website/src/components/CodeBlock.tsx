import { codeToHtml } from 'shiki';

interface CodeBlockProps {
  code: string;
  language?: string;
  filename?: string;
}

export async function CodeBlock({ code, language = 'typescript', filename }: CodeBlockProps) {
  const html = await codeToHtml(code, {
    lang: language,
    themes: {
      light: 'github-light',
      dark: 'github-dark',
    },
  });

  return (
    <div className="rounded-lg overflow-hidden border border-[var(--color-border)]">
      {filename && (
        <div className="bg-[var(--color-bg-secondary)] px-4 py-2 text-xs font-mono text-[var(--color-text-secondary)] border-b border-[var(--color-border)]">
          {filename}
        </div>
      )}
      <div
        className="text-sm leading-relaxed [&_pre]:p-4 [&_pre]:overflow-x-auto [&_pre]:!bg-[var(--color-code-bg)]"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </div>
  );
}
