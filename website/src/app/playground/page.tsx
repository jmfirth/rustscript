import Link from 'next/link';

export const metadata = {
  title: 'Playground - RustScript',
};

export default function PlaygroundPage() {
  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-20">
      <div className="max-w-2xl mx-auto text-center">
        <h1 className="text-4xl font-bold mb-4">Playground</h1>
        <p className="text-lg text-[var(--color-text-secondary)] mb-8">
          An interactive RustScript playground is coming soon. Write{' '}
          <code className="bg-[var(--color-code-bg)] px-1.5 py-0.5 rounded text-sm font-mono">
            .rts
          </code>{' '}
          code in the browser and see the compiled Rust output in real time.
        </p>
        <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-code-bg)] p-12 text-[var(--color-text-secondary)] font-mono text-sm">
          // playground will appear here
        </div>
        <div className="mt-8">
          <Link
            href="/docs"
            className="text-[var(--color-accent)] hover:underline font-medium"
          >
            Read the docs while you wait &rarr;
          </Link>
        </div>
      </div>
    </div>
  );
}
