import Link from 'next/link';

const crateInfo: Record<string, { name: string; description: string }> = {
  serde: {
    name: 'serde',
    description: 'Serialization and deserialization framework. JSON, TOML, YAML, and more.',
  },
  axum: {
    name: 'axum',
    description: 'Ergonomic web framework built on tokio and hyper.',
  },
  tokio: {
    name: 'tokio',
    description: 'Async runtime for writing reliable, asynchronous applications.',
  },
  clap: {
    name: 'clap',
    description: 'Command line argument parser with derive macros.',
  },
  reqwest: {
    name: 'reqwest',
    description: 'HTTP client with async/await support.',
  },
};

export function generateStaticParams() {
  return Object.keys(crateInfo).map((name) => ({
    crate: name,
  }));
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ crate: string }>;
}) {
  const { crate: crateName } = await params;
  const info = crateInfo[crateName];
  return {
    title: info
      ? `${info.name} - RustScript Crate Docs`
      : 'RustScript Crate Docs',
  };
}

export default async function CratePage({
  params,
}: {
  params: Promise<{ crate: string }>;
}) {
  const { crate: crateName } = await params;
  const info = crateInfo[crateName];

  if (!info) {
    return (
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-20 text-center">
        <h1 className="text-3xl font-bold mb-4">Crate not found</h1>
        <Link href="/crates" className="text-[var(--color-accent)] hover:underline">
          Back to crates
        </Link>
      </div>
    );
  }

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-20">
      <div className="mb-4">
        <Link
          href="/crates"
          className="text-sm text-[var(--color-text-secondary)] hover:text-[var(--color-text)] transition-colors"
        >
          &larr; All Crates
        </Link>
      </div>
      <h1 className="text-3xl font-bold font-mono mb-4">{info.name}</h1>
      <p className="text-lg text-[var(--color-text-secondary)] mb-8">
        {info.description}
      </p>
      <div className="px-4 py-3 rounded-lg bg-[var(--color-bg-secondary)] border border-[var(--color-border)]">
        <p className="text-sm text-[var(--color-text-secondary)]">
          Detailed documentation for the{' '}
          <code className="bg-[var(--color-code-bg)] px-1.5 py-0.5 rounded font-mono">
            {info.name}
          </code>{' '}
          crate integration is coming soon.
        </p>
      </div>

      <div className="mt-8">
        <h2 className="text-xl font-semibold mb-4">Usage</h2>
        <pre className="bg-[var(--color-code-bg)] p-4 rounded-lg overflow-x-auto text-sm font-mono border border-[var(--color-border)]">
          <code>{`import { /* ... */ } from "${info.name}";`}</code>
        </pre>
      </div>
    </div>
  );
}
