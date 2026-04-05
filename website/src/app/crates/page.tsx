import Link from 'next/link';

export const metadata = {
  title: 'Crate Documentation - RustScript',
};

const crates = [
  {
    name: 'serde',
    description: 'Serialization and deserialization framework. JSON, TOML, YAML, and more.',
  },
  {
    name: 'axum',
    description: 'Ergonomic web framework built on tokio and hyper.',
  },
  {
    name: 'tokio',
    description: 'Async runtime for writing reliable, asynchronous applications.',
  },
  {
    name: 'clap',
    description: 'Command line argument parser with derive macros.',
  },
  {
    name: 'reqwest',
    description: 'HTTP client with async/await support.',
  },
];

export default function CratesIndexPage() {
  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-20">
      <h1 className="text-3xl font-bold mb-4">Crate Documentation</h1>
      <p className="text-lg text-[var(--color-text-secondary)] mb-8 leading-relaxed">
        RustScript provides TypeScript-style bindings for popular Rust crates.
        Import them with standard TypeScript import syntax and use them with
        familiar APIs.
      </p>
      <p className="text-sm text-[var(--color-text-secondary)] mb-8 px-4 py-3 rounded-lg bg-[var(--color-bg-secondary)] border border-[var(--color-border)]">
        Crate documentation is coming soon. Below are the planned integrations.
      </p>

      <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-4">
        {crates.map((crate) => (
          <Link
            key={crate.name}
            href={`/crates/${crate.name}`}
            className="block p-6 rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] transition-colors"
          >
            <h3 className="font-semibold font-mono mb-2">{crate.name}</h3>
            <p className="text-sm text-[var(--color-text-secondary)]">
              {crate.description}
            </p>
          </Link>
        ))}
      </div>
    </div>
  );
}
