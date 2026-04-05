'use client';

import Link from 'next/link';
import { useState, useEffect, useCallback } from 'react';
import { useCompiler } from '@/lib/use-compiler';
import type { TranslatedItem } from '@/lib/rsc-compiler';

const RUSTDOC_PROXY =
  process.env.NEXT_PUBLIC_RUSTDOC_PROXY || 'https://docs.rs';

type LoadingPhase =
  | 'init'
  | 'fetching'
  | 'decompressing'
  | 'translating'
  | 'done'
  | 'error';

interface GroupedItems {
  functions: TranslatedItem[];
  structs: TranslatedItem[];
  traits: TranslatedItem[];
  enums: TranslatedItem[];
}

function groupItems(items: TranslatedItem[]): GroupedItems {
  const groups: GroupedItems = {
    functions: [],
    structs: [],
    traits: [],
    enums: [],
  };
  for (const item of items) {
    switch (item.kind) {
      case 'function':
        groups.functions.push(item);
        break;
      case 'struct':
        groups.structs.push(item);
        break;
      case 'trait':
        groups.traits.push(item);
        break;
      case 'enum':
        groups.enums.push(item);
        break;
    }
  }
  // Sort each group alphabetically
  for (const list of Object.values(groups)) {
    list.sort((a: TranslatedItem, b: TranslatedItem) => a.name.localeCompare(b.name));
  }
  return groups;
}

function ItemSection({
  title,
  items,
}: {
  title: string;
  items: TranslatedItem[];
}) {
  if (items.length === 0) return null;

  return (
    <section className="mb-10">
      <h2 className="text-xl font-semibold mb-4 pb-2 border-b border-[var(--color-border)]">
        {title}{' '}
        <span className="text-sm font-normal text-[var(--color-text-secondary)]">
          ({items.length})
        </span>
      </h2>
      <div className="space-y-6">
        {items.map((item) => (
          <DocItem key={`${item.module ?? ''}::${item.name}`} item={item} />
        ))}
      </div>
    </section>
  );
}

/** Strip markdown code fences from translator output */
function stripCodeFences(sig: string): string {
  return sig
    .replace(/^```\w*\n?/gm, '')
    .replace(/^```$/gm, '')
    .trim();
}

/** Filter out trait impl methods and internal items.
 *  Keep only the crate's own public API: direct types, traits, and inherent methods. */
function filterItems(items: TranslatedItem[]): TranslatedItem[] {
  return items.filter(item => {
    // Filter out items starting with underscore (internal)
    if (item.name.startsWith('_')) {
      return false;
    }
    // Filter out trait impl methods (From, Into, Borrow, Display, etc.)
    if (item.is_trait_impl) {
      return false;
    }
    // Filter out all method-style functions (Type.method or Trait.method)
    // These are either trait method definitions or inherent impl methods.
    // A RustScript dev wants to see types and free functions, not method lists.
    if (item.kind === 'function') {
      const sig = stripCodeFences(item.signature);
      if (/function\s+\S+\.\S+/.test(sig)) {
        return false;
      }
    }
    return true;
  });
}

/** Deduplicate items by name + signature (trait methods appear once per impl) */
function deduplicateItems(items: TranslatedItem[]): TranslatedItem[] {
  const seen = new Set<string>();
  return items.filter(item => {
    const key = `${item.kind}:${item.name}:${stripCodeFences(item.signature)}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function DocItem({ item }: { item: TranslatedItem }) {
  const signature = stripCodeFences(item.signature);

  return (
    <div className="border border-[var(--color-border)] rounded-lg overflow-hidden">
      <div className="px-4 py-3 bg-[var(--color-bg-secondary)] border-b border-[var(--color-border)] flex items-center gap-3">
        <span className="inline-block px-2 py-0.5 rounded text-xs font-mono font-medium bg-[var(--color-accent)] text-white">
          {item.kind}
        </span>
        <span className="font-mono font-semibold text-sm">{item.name}</span>
        {item.module && (
          <span className="text-xs text-[var(--color-text-secondary)] font-mono">
            {item.module}
          </span>
        )}
      </div>
      <pre className="px-4 py-3 overflow-x-auto text-sm font-mono bg-[var(--color-code-bg)]">
        <code className="rustscript">{signature}</code>
      </pre>
      {item.docs && (
        <div
          className="px-4 py-3 text-sm text-[var(--color-text-secondary)] border-t border-[var(--color-border)] leading-relaxed [&_code]:bg-[var(--color-code-bg)] [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-xs [&_code]:font-mono [&_a]:text-[var(--color-accent)] [&_a]:underline"
          dangerouslySetInnerHTML={{ __html: item.docs }}
        />
      )}
    </div>
  );
}

export function CrateDocsViewer({ crateName }: { crateName: string }) {
  const { ready, translateRustdoc } = useCompiler();
  const [phase, setPhase] = useState<LoadingPhase>('init');
  const [error, setError] = useState<string | null>(null);
  const [items, setItems] = useState<TranslatedItem[] | null>(null);
  const [version, setVersion] = useState('latest');
  const [versionInput, setVersionInput] = useState('latest');
  const [isCorsError, setIsCorsError] = useState(false);

  const fetchAndTranslate = useCallback(
    async (crate: string, ver: string) => {
      if (!ready) return;

      setPhase('fetching');
      setError(null);
      setItems(null);
      setIsCorsError(false);

      try {
        const url = `${RUSTDOC_PROXY}/crate/${crate}/${ver}/json.gz`;
        const response = await fetch(url);

        if (!response.ok) {
          throw new Error(
            `Failed to fetch documentation: HTTP ${response.status}`
          );
        }

        if (!response.body) {
          throw new Error('Response has no body');
        }

        setPhase('decompressing');
        const ds = new DecompressionStream('gzip');
        const decompressed = response.body.pipeThrough(ds);
        const text = await new Response(decompressed).text();

        setPhase('translating');
        const translated = await translateRustdoc(text);
        setItems(translated);
        setPhase('done');
      } catch (err) {
        const message =
          err instanceof Error ? err.message : 'Unknown error occurred';
        // Detect CORS-related failures
        if (
          message.includes('Failed to fetch') ||
          message.includes('CORS') ||
          message.includes('NetworkError') ||
          message.includes('Network request failed')
        ) {
          setIsCorsError(true);
        }
        setError(message);
        setPhase('error');
      }
    },
    [ready, translateRustdoc]
  );

  useEffect(() => {
    if (ready) {
      fetchAndTranslate(crateName, version);
    }
  }, [ready, crateName, version, fetchAndTranslate]);

  const handleVersionSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const trimmed = versionInput.trim();
    if (trimmed && trimmed !== version) {
      setVersion(trimmed);
    }
  };

  const grouped = items ? groupItems(deduplicateItems(filterItems(items))) : null;

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-20">
      {/* Breadcrumb */}
      <div className="mb-4">
        <Link
          href="/crates"
          className="text-sm text-[var(--color-text-secondary)] hover:text-[var(--color-text)] transition-colors"
        >
          &larr; All Crates
        </Link>
      </div>

      {/* Header */}
      <div className="flex items-start justify-between gap-6 mb-8 flex-wrap">
        <div>
          <h1 className="text-3xl font-bold font-mono mb-2">{crateName}</h1>
          <p className="text-sm text-[var(--color-text-secondary)]">
            Rust crate documentation translated to RustScript syntax
          </p>
        </div>

        {/* Version selector */}
        <form
          onSubmit={handleVersionSubmit}
          className="flex items-center gap-2"
        >
          <label
            htmlFor="version"
            className="text-sm text-[var(--color-text-secondary)]"
          >
            Version:
          </label>
          <input
            id="version"
            type="text"
            value={versionInput}
            onChange={(e) => setVersionInput(e.target.value)}
            className="w-28 px-3 py-1.5 rounded border border-[var(--color-border)] bg-[var(--color-bg-secondary)] text-[var(--color-text)] font-mono text-sm focus:outline-none focus:border-[var(--color-accent)]"
          />
          <button
            type="submit"
            className="px-3 py-1.5 rounded text-sm bg-[var(--color-accent)] text-white hover:opacity-90 transition-opacity"
          >
            Go
          </button>
        </form>
      </div>

      {/* Usage example */}
      <div className="mb-8">
        <pre className="bg-[var(--color-code-bg)] px-4 py-3 rounded-lg overflow-x-auto text-sm font-mono border border-[var(--color-border)]">
          <code className="rustscript">{`import { /* ... */ } from "${crateName}";`}</code>
        </pre>
      </div>

      {/* Loading states */}
      {phase === 'init' && (
        <StatusMessage>Initializing compiler...</StatusMessage>
      )}
      {phase === 'fetching' && (
        <StatusMessage>
          Fetching documentation for{' '}
          <code className="font-mono">{crateName}</code>...
        </StatusMessage>
      )}
      {phase === 'decompressing' && (
        <StatusMessage>Decompressing...</StatusMessage>
      )}
      {phase === 'translating' && (
        <StatusMessage>
          Translating to RustScript syntax...
        </StatusMessage>
      )}

      {/* Error state */}
      {phase === 'error' && isCorsError && (
        <CorsErrorMessage crateName={crateName} />
      )}
      {phase === 'error' && !isCorsError && (
        <div className="px-4 py-3 rounded-lg bg-red-500/10 border border-red-500/30 text-sm">
          <p className="font-medium mb-1">
            Failed to load documentation for {crateName}
          </p>
          <p className="text-[var(--color-text-secondary)]">{error}</p>
          <button
            onClick={() => fetchAndTranslate(crateName, version)}
            className="mt-3 px-4 py-1.5 rounded text-sm bg-[var(--color-accent)] text-white hover:opacity-90 transition-opacity"
          >
            Retry
          </button>
        </div>
      )}

      {/* Results */}
      {phase === 'done' && grouped && (
        <>
          {/* Summary */}
          <div className="mb-8 flex flex-wrap gap-4">
            {grouped.functions.length > 0 && (
              <SummaryBadge
                label="Functions"
                count={grouped.functions.length}
                href="#functions"
              />
            )}
            {grouped.structs.length > 0 && (
              <SummaryBadge
                label="Structs"
                count={grouped.structs.length}
                href="#structs"
              />
            )}
            {grouped.traits.length > 0 && (
              <SummaryBadge
                label="Traits"
                count={grouped.traits.length}
                href="#traits"
              />
            )}
            {grouped.enums.length > 0 && (
              <SummaryBadge
                label="Enums"
                count={grouped.enums.length}
                href="#enums"
              />
            )}
          </div>

          <div id="functions">
            <ItemSection title="Functions" items={grouped.functions} />
          </div>
          <div id="structs">
            <ItemSection title="Structs" items={grouped.structs} />
          </div>
          <div id="traits">
            <ItemSection title="Traits" items={grouped.traits} />
          </div>
          <div id="enums">
            <ItemSection title="Enums" items={grouped.enums} />
          </div>

          {items && items.length === 0 && (
            <div className="text-center py-12 text-[var(--color-text-secondary)]">
              <p>No translatable items found in this crate.</p>
              <p className="text-sm mt-2">
                The crate may not have public API items, or the translation may
                not support its structure yet.
              </p>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function StatusMessage({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-3 px-4 py-3 rounded-lg bg-[var(--color-bg-secondary)] border border-[var(--color-border)] text-sm text-[var(--color-text-secondary)]">
      <svg
        className="animate-spin h-4 w-4 shrink-0"
        viewBox="0 0 24 24"
        fill="none"
      >
        <circle
          className="opacity-25"
          cx="12"
          cy="12"
          r="10"
          stroke="currentColor"
          strokeWidth="4"
        />
        <path
          className="opacity-75"
          fill="currentColor"
          d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
        />
      </svg>
      <span>{children}</span>
    </div>
  );
}

function CorsErrorMessage({ crateName }: { crateName: string }) {
  return (
    <div className="px-6 py-5 rounded-lg bg-[var(--color-bg-secondary)] border border-[var(--color-border)]">
      <h3 className="font-semibold mb-3">
        Unable to fetch documentation for{' '}
        <code className="font-mono bg-[var(--color-code-bg)] px-1.5 py-0.5 rounded">
          {crateName}
        </code>
      </h3>
      <p className="text-sm text-[var(--color-text-secondary)] mb-4">
        This feature requires the RustScript docs proxy to be deployed. Direct
        requests to docs.rs are blocked by CORS policy.
      </p>
      <div className="text-sm text-[var(--color-text-secondary)] space-y-2">
        <p className="font-medium text-[var(--color-text)]">
          In the meantime, you can view translated documentation by:
        </p>
        <ol className="list-decimal list-inside space-y-1 ml-2">
          <li>
            Running:{' '}
            <code className="bg-[var(--color-code-bg)] px-1.5 py-0.5 rounded font-mono text-xs">
              cargo +nightly doc --output-format json -p {crateName}
            </code>
          </li>
          <li>Loading the JSON file in the playground</li>
        </ol>
      </div>
      <p className="text-xs text-[var(--color-text-secondary)] mt-4">
        See{' '}
        <code className="bg-[var(--color-code-bg)] px-1 py-0.5 rounded font-mono">
          website/worker/README.md
        </code>{' '}
        for proxy deployment instructions.
      </p>
    </div>
  );
}

function SummaryBadge({
  label,
  count,
  href,
}: {
  label: string;
  count: number;
  href: string;
}) {
  return (
    <a
      href={href}
      className="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] transition-colors text-sm"
    >
      <span className="font-medium">{label}</span>
      <span className="text-[var(--color-text-secondary)]">{count}</span>
    </a>
  );
}
