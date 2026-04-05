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

// ---------------------------------------------------------------------------
// Data model for type-centric view
// ---------------------------------------------------------------------------

interface TypeSection {
  name: string;
  kind: 'struct' | 'trait' | 'enum';
  signature: string;
  docs: string | null;
  methods: TranslatedItem[];
}

interface GroupedView {
  freeFunctions: TranslatedItem[];
  types: TypeSection[];
}

/** Build a type-centric view: types with nested methods + free functions. */
function buildGroupedView(items: TranslatedItem[]): GroupedView {
  // 1. Collect type entities (structs, traits, enums)
  const typeMap = new Map<string, TypeSection>();
  for (const item of items) {
    if (item.kind === 'struct' || item.kind === 'trait' || item.kind === 'enum') {
      typeMap.set(item.name, {
        name: item.name,
        kind: item.kind,
        signature: item.signature,
        docs: item.docs,
        methods: [],
      });
    }
  }

  // 2. Assign functions to their parent type or free functions
  const freeFunctions: TranslatedItem[] = [];
  for (const item of items) {
    if (item.kind !== 'function') continue;
    if (item.parent_type && typeMap.has(item.parent_type)) {
      typeMap.get(item.parent_type)!.methods.push(item);
    } else if (!item.parent_type) {
      freeFunctions.push(item);
    }
    // Functions with parent_type not matching any known type are dropped
    // (they belong to internal/private types)
  }

  // 3. Sort
  freeFunctions.sort((a, b) => a.name.localeCompare(b.name));
  const types = Array.from(typeMap.values()).sort((a, b) => a.name.localeCompare(b.name));
  for (const t of types) {
    t.methods.sort((a, b) => a.name.localeCompare(b.name));
  }

  return { freeFunctions, types };
}

// ---------------------------------------------------------------------------
// Utility functions (preserved from original)
// ---------------------------------------------------------------------------

/** Strip markdown code fences and doc prefix from translator output.
 *  The translator emits "docs\n---\n```rustscript\nsignature\n```".
 *  We want only the signature. Find the LAST ```rustscript fence. */
function stripCodeFences(sig: string): string {
  const lastFence = sig.lastIndexOf('```rustscript');
  const cleaned = lastFence >= 0 ? sig.substring(lastFence) : sig;
  return cleaned
    .replace(/^```\w*\n?/gm, '')
    .replace(/^```$/gm, '')
    .trim();
}

/** Pretty-print a long function signature across multiple lines */
function formatSignature(sig: string): string {
  if (sig.length < 80) return sig;
  return sig.replace(
    /^((?:async\s+)?function\s+\S+)\(([^)]{40,})\)(:\s*.+)?$/,
    (_match, prefix, params, ret) => {
      const paramList = params.split(',').map((p: string) => `  ${p.trim()}`).join(',\n');
      return `${prefix}(\n${paramList}\n)${ret || ''}`;
    }
  );
}

/** Filter out trait impl methods and internal items.
 *  Keep public API items. Methods with parent_type are kept (nested under their type). */
function filterItems(items: TranslatedItem[]): TranslatedItem[] {
  return items.filter(item => {
    // Filter out items starting with underscore (internal)
    if (item.name.startsWith('_')) {
      return false;
    }
    // Filter out trait impl methods (Clone, Debug, From, Into, etc.)
    if (item.is_trait_impl) {
      return false;
    }
    // Only show items that are part of the crate's public API,
    // OR methods whose parent_type matches a public API item
    if (!item.is_public_api && !item.parent_type) {
      return false;
    }
    return true;
  });
}

/** Escape HTML entities */
function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/** Simple syntax highlighting for RustScript signatures */
function highlightSignature(sig: string): string {
  return escapeHtml(sig)
    .replace(/\b(function|async|class|interface|enum|type|const|let|extends|implements|throws)\b/g,
      '<span class="crate-keyword">$1</span>')
    .replace(/\b(string|boolean|void|never|number)\b/g,
      '<span class="crate-type">$1</span>')
    .replace(/\b(i8|i16|i32|i64|u8|u16|u32|u64|f32|f64|usize|isize)\b/g,
      '<span class="crate-type">$1</span>')
    .replace(/&quot;([^&]*)&quot;/g, '<span class="crate-string">"$1"</span>');
}

/** Extract the first sentence from docs, stripping markdown/HTML noise */
function firstSentence(docs: string | null): string | null {
  if (!docs) return null;
  const plain = docs.replace(/<[^>]+>/g, '');
  const match = plain.match(/^(.+?[.!])\s/);
  const sentence = match ? match[1] : plain.split('\n')[0];
  const trimmed = sentence?.trim();
  if (!trimmed || trimmed.length < 3) return null;
  return trimmed.length > 120 ? trimmed.substring(0, 117) + '...' : trimmed;
}

/** Deduplicate items by kind + name + parent_type, keeping the entry with the most docs */
function deduplicateItems(items: TranslatedItem[]): TranslatedItem[] {
  const best = new Map<string, TranslatedItem>();
  for (const item of items) {
    const key = `${item.kind}:${item.name}:${item.parent_type ?? ''}`;
    const existing = best.get(key);
    if (!existing || (item.docs?.length ?? 0) > (existing.docs?.length ?? 0)) {
      best.set(key, item);
    }
  }
  return Array.from(best.values());
}

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

/** Render a single item (function/type) as highlighted HTML with optional doc comment */
function renderItemHtml(item: TranslatedItem): string {
  const sig = formatSignature(stripCodeFences(item.signature));
  const summary = firstSentence(item.docs);
  const highlighted = highlightSignature(sig);
  const commentHtml = summary
    ? `<span class="crate-comment">// ${escapeHtml(summary)}</span>\n`
    : '';
  return `${commentHtml}${highlighted}`;
}

/** Render a list of items as a code block */
function ItemCodeBlock({ items }: { items: TranslatedItem[] }) {
  if (items.length === 0) return null;
  return (
    <pre className="bg-[var(--color-code-bg)] rounded-lg p-4 overflow-x-auto text-sm font-mono leading-relaxed">
      <code dangerouslySetInnerHTML={{ __html: items.map(renderItemHtml).join('\n\n') }} />
    </pre>
  );
}

// ---------------------------------------------------------------------------
// Section components
// ---------------------------------------------------------------------------

function TypeSectionView({ section }: { section: TypeSection }) {
  const sig = formatSignature(stripCodeFences(section.signature));
  const summary = firstSentence(section.docs);
  const kindLabel = section.kind === 'struct' ? 'class' : section.kind;

  return (
    <section id={section.name} className="mb-10 scroll-mt-24">
      <h3 className="text-lg font-semibold font-mono mb-1 flex items-center gap-2">
        <a href={`#${section.name}`} className="hover:text-[var(--color-accent)] transition-colors">
          <span dangerouslySetInnerHTML={{ __html: highlightSignature(`${kindLabel} ${sig.replace(/^(class|interface|enum|type)\s+/, '')}`) }} />
        </a>
      </h3>
      {summary && (
        <p className="text-sm text-[var(--color-text-secondary)] mb-3 ml-0.5">
          {summary}
        </p>
      )}

      {section.methods.length > 0 && (
        <details open className="mt-2">
          <summary className="cursor-pointer text-sm font-medium text-[var(--color-text-secondary)] mb-2 select-none hover:text-[var(--color-text)] transition-colors">
            Methods ({section.methods.length})
          </summary>
          <ItemCodeBlock items={section.methods} />
        </details>
      )}

      {section.methods.length === 0 && (
        <p className="text-xs text-[var(--color-text-secondary)] ml-0.5 italic">
          No public methods
        </p>
      )}
    </section>
  );
}

function FreeFunctionsSection({ items }: { items: TranslatedItem[] }) {
  if (items.length === 0) return null;
  return (
    <section id="functions" className="mb-10 scroll-mt-24">
      <h3 className="text-lg font-semibold mb-3 pb-2 border-b border-[var(--color-border)]">
        Free Functions{' '}
        <span className="text-sm font-normal text-[var(--color-text-secondary)]">
          ({items.length})
        </span>
      </h3>
      <ItemCodeBlock items={items} />
    </section>
  );
}

// ---------------------------------------------------------------------------
// Table of contents
// ---------------------------------------------------------------------------

function TableOfContents({ grouped }: { grouped: GroupedView }) {
  const structs = grouped.types.filter(t => t.kind === 'struct');
  const traits = grouped.types.filter(t => t.kind === 'trait');
  const enums = grouped.types.filter(t => t.kind === 'enum');

  return (
    <div className="mb-8 space-y-3">
      {/* Category jump links */}
      <div className="flex flex-wrap gap-4">
        {grouped.freeFunctions.length > 0 && (
          <SummaryBadge label="Free Functions" count={grouped.freeFunctions.length} href="#functions" />
        )}
        {structs.length > 0 && (
          <SummaryBadge label="Structs" count={structs.length} href="#structs-section" />
        )}
        {traits.length > 0 && (
          <SummaryBadge label="Traits" count={traits.length} href="#traits-section" />
        )}
        {enums.length > 0 && (
          <SummaryBadge label="Enums" count={enums.length} href="#enums-section" />
        )}
      </div>

      {/* Per-type anchor links */}
      {grouped.types.length > 0 && (
        <div className="flex flex-wrap gap-x-2 gap-y-1 text-sm font-mono">
          {grouped.types.map(t => (
            <a
              key={t.name}
              href={`#${t.name}`}
              className="text-[var(--color-text-secondary)] hover:text-[var(--color-accent)] transition-colors"
            >
              {t.name}
            </a>
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

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

  const grouped = items ? buildGroupedView(deduplicateItems(filterItems(items))) : null;

  const structs = grouped ? grouped.types.filter(t => t.kind === 'struct') : [];
  const traits = grouped ? grouped.types.filter(t => t.kind === 'trait') : [];
  const enums = grouped ? grouped.types.filter(t => t.kind === 'enum') : [];

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
          <TableOfContents grouped={grouped} />

          {/* Free Functions */}
          <FreeFunctionsSection items={grouped.freeFunctions} />

          {/* Structs */}
          {structs.length > 0 && (
            <div id="structs-section" className="scroll-mt-24">
              <h2 className="text-xl font-semibold mb-6 pb-2 border-b border-[var(--color-border)]">
                Structs{' '}
                <span className="text-sm font-normal text-[var(--color-text-secondary)]">
                  ({structs.length})
                </span>
              </h2>
              {structs.map(t => (
                <TypeSectionView key={t.name} section={t} />
              ))}
            </div>
          )}

          {/* Traits */}
          {traits.length > 0 && (
            <div id="traits-section" className="scroll-mt-24">
              <h2 className="text-xl font-semibold mb-6 pb-2 border-b border-[var(--color-border)]">
                Traits{' '}
                <span className="text-sm font-normal text-[var(--color-text-secondary)]">
                  ({traits.length})
                </span>
              </h2>
              {traits.map(t => (
                <TypeSectionView key={t.name} section={t} />
              ))}
            </div>
          )}

          {/* Enums */}
          {enums.length > 0 && (
            <div id="enums-section" className="scroll-mt-24">
              <h2 className="text-xl font-semibold mb-6 pb-2 border-b border-[var(--color-border)]">
                Enums{' '}
                <span className="text-sm font-normal text-[var(--color-text-secondary)]">
                  ({enums.length})
                </span>
              </h2>
              {enums.map(t => (
                <TypeSectionView key={t.name} section={t} />
              ))}
            </div>
          )}

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

// ---------------------------------------------------------------------------
// Shared UI components
// ---------------------------------------------------------------------------

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
