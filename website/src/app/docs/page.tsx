import Link from 'next/link';

export const metadata = {
  title: 'Documentation - RustScript',
};

export default function DocsIndexPage() {
  return (
    <div>
      <h1 className="text-3xl font-bold mb-4">RustScript Documentation</h1>
      <p className="text-lg text-[var(--color-text-secondary)] mb-8 leading-relaxed">
        RustScript is a TypeScript-native authoring language that compiles to
        idiomatic Rust. Write the TypeScript you already know. Ship native
        binaries.
      </p>

      <div className="grid sm:grid-cols-2 gap-4">
        <Link
          href="/docs/getting-started/installation"
          className="block p-6 rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] transition-colors"
        >
          <h3 className="font-semibold mb-2">Installation</h3>
          <p className="text-sm text-[var(--color-text-secondary)]">
            Install the RustScript compiler and create your first project.
          </p>
        </Link>
        <Link
          href="/playground"
          className="block p-6 rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] transition-colors"
        >
          <h3 className="font-semibold mb-2">Playground</h3>
          <p className="text-sm text-[var(--color-text-secondary)]">
            Try RustScript in the browser without installing anything.
          </p>
        </Link>
      </div>
    </div>
  );
}
