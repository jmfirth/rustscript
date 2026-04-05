'use client';

import { useRouter, useSearchParams } from 'next/navigation';
import { useState, useCallback, Suspense } from 'react';
import { CrateDocsViewer } from '@/components/CrateDocsViewer';

const popularCrates = [
  { name: 'axum', description: 'Ergonomic web framework built on tokio and hyper.' },
  { name: 'serde', description: 'Serialization and deserialization framework.' },
  { name: 'tokio', description: 'Async runtime for reliable, asynchronous applications.' },
  { name: 'clap', description: 'Command line argument parser with derive macros.' },
  { name: 'reqwest', description: 'HTTP client with async/await support.' },
  { name: 'sqlx', description: 'Async SQL toolkit with compile-time checked queries.' },
];

function CratesContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const activeCrate = searchParams.get('name');
  const activeVersion = searchParams.get('version') || 'latest';

  const [crateName, setCrateName] = useState(activeCrate || '');
  const [version, setVersion] = useState(activeVersion);

  const handleSubmit = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      const trimmed = crateName.trim().toLowerCase();
      if (trimmed) {
        const params = new URLSearchParams({ name: trimmed });
        if (version && version !== 'latest') {
          params.set('version', version);
        }
        router.push(`/crates?${params.toString()}`);
      }
    },
    [crateName, version, router]
  );

  const handleCrateClick = useCallback(
    (name: string) => {
      setCrateName(name);
      router.push(`/crates?name=${name}`);
    },
    [router]
  );

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-20">
      <h1 className="text-3xl font-bold mb-4">Crate Documentation</h1>
      <p className="text-lg text-[var(--color-text-secondary)] mb-8 leading-relaxed">
        Rust crate APIs translated to RustScript syntax.
        Enter any crate from{' '}
        <a
          href="https://crates.io"
          className="text-[var(--color-accent)] hover:underline"
          target="_blank"
          rel="noopener noreferrer"
        >
          crates.io
        </a>.
      </p>

      <form onSubmit={handleSubmit} className="mb-10">
        <div className="flex gap-3">
          <input
            type="text"
            value={crateName}
            onChange={(e) => setCrateName(e.target.value)}
            placeholder="Crate name (e.g. axum, serde, tokio)"
            className="flex-1 px-4 py-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] text-[var(--color-text)] placeholder:text-[var(--color-text-secondary)] focus:outline-none focus:border-[var(--color-accent)] font-mono text-sm"
          />
          <input
            type="text"
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            placeholder="latest"
            className="w-28 px-4 py-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-secondary)] text-[var(--color-text)] placeholder:text-[var(--color-text-secondary)] focus:outline-none focus:border-[var(--color-accent)] font-mono text-sm"
          />
          <button
            type="submit"
            className="px-6 py-3 rounded-lg bg-[var(--color-accent)] text-white font-medium hover:opacity-90 transition-opacity text-sm"
          >
            View Docs
          </button>
        </div>
      </form>

      {/* Show viewer when a crate is selected */}
      {activeCrate && (
        <CrateDocsViewer crateName={activeCrate} version={activeVersion} />
      )}

      {/* Show popular crates when no crate is selected */}
      {!activeCrate && (
        <>
          <h2 className="text-xl font-semibold mb-4">Popular Crates</h2>
          <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {popularCrates.map((c) => (
              <button
                key={c.name}
                onClick={() => handleCrateClick(c.name)}
                className="block p-6 rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] transition-colors text-left"
              >
                <h3 className="font-semibold font-mono mb-2">{c.name}</h3>
                <p className="text-sm text-[var(--color-text-secondary)]">
                  {c.description}
                </p>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

export default function CratesPage() {
  return (
    <Suspense>
      <CratesContent />
    </Suspense>
  );
}
