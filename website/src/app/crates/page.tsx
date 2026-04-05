'use client';

import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useState, useCallback } from 'react';

const popularCrates = [
  {
    name: 'axum',
    description: 'Ergonomic web framework built on tokio and hyper.',
  },
  {
    name: 'serde',
    description: 'Serialization and deserialization framework. JSON, TOML, YAML, and more.',
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
  {
    name: 'sqlx',
    description: 'Async SQL toolkit with compile-time checked queries.',
  },
];

export default function CratesIndexPage() {
  const [search, setSearch] = useState('');
  const router = useRouter();

  const filtered = popularCrates.filter((c) =>
    c.name.toLowerCase().includes(search.toLowerCase())
  );

  const handleSubmit = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      const trimmed = search.trim().toLowerCase();
      if (trimmed) {
        router.push(`/crates/${trimmed}`);
      }
    },
    [search, router]
  );

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-20">
      <h1 className="text-3xl font-bold mb-4">Crate Documentation</h1>
      <p className="text-lg text-[var(--color-text-secondary)] mb-8 leading-relaxed">
        Browse Rust crate documentation translated to RustScript syntax.
        Search for any crate from{' '}
        <a
          href="https://crates.io"
          className="text-[var(--color-accent)] hover:underline"
          target="_blank"
          rel="noopener noreferrer"
        >
          crates.io
        </a>{' '}
        or pick from the popular crates below.
      </p>

      <form onSubmit={handleSubmit} className="mb-10">
        <div className="flex gap-3">
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search crates (e.g. axum, serde, tokio)..."
            className="flex-1 px-4 py-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] text-[var(--color-text)] placeholder:text-[var(--color-text-secondary)] focus:outline-none focus:border-[var(--color-accent)] font-mono text-sm"
          />
          <button
            type="submit"
            className="px-6 py-3 rounded-lg bg-[var(--color-accent)] text-white font-medium hover:opacity-90 transition-opacity text-sm"
          >
            View Docs
          </button>
        </div>
      </form>

      <h2 className="text-xl font-semibold mb-4">Popular Crates</h2>
      <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-4">
        {filtered.map((crate) => (
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
        {filtered.length === 0 && search && (
          <div className="col-span-full text-center py-8">
            <p className="text-[var(--color-text-secondary)] mb-3">
              No matching popular crates. Press{' '}
              <kbd className="px-2 py-0.5 rounded bg-[var(--color-bg-secondary)] border border-[var(--color-border)] font-mono text-xs">
                Enter
              </kbd>{' '}
              or click <strong>View Docs</strong> to look up{' '}
              <code className="bg-[var(--color-code-bg)] px-1.5 py-0.5 rounded font-mono">
                {search}
              </code>{' '}
              from docs.rs.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
