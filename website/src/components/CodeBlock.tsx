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
    defaultColor: false,
  });

  return (
    <div className="code-block-landing rounded-lg overflow-hidden border border-[var(--color-border)] h-full flex flex-col">
      {filename && (
        <div className="bg-[var(--color-bg-secondary)] px-4 py-2 text-xs font-mono text-[var(--color-text-secondary)] border-b border-[var(--color-border)]">
          {filename}
        </div>
      )}
      <div
        className="flex-1 text-sm leading-relaxed bg-[var(--color-code-bg)] [&_pre]:p-4 [&_pre]:h-full [&_pre]:overflow-x-auto"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </div>
  );
}
