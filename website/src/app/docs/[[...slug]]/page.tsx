import Link from 'next/link';
import { notFound } from 'next/navigation';
import { docPages, getDocContent } from '@/lib/mdx';

export function generateStaticParams() {
  return [
    { slug: [] }, // /docs index
    ...docPages.map((page) => ({
      slug: page.slug,
    })),
  ];
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ slug?: string[] }>;
}) {
  const { slug } = await params;
  if (!slug || slug.length === 0) {
    return { title: 'Documentation - RustScript' };
  }

  const page = docPages.find(
    (p) => p.slug.join('/') === slug.join('/')
  );

  return {
    title: page ? `${page.title} - RustScript Docs` : 'RustScript Docs',
    description: page?.description,
  };
}

function DocsIndex() {
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

export default async function DocPage({
  params,
}: {
  params: Promise<{ slug?: string[] }>;
}) {
  const { slug } = await params;

  // No slug = docs index
  if (!slug || slug.length === 0) {
    return <DocsIndex />;
  }

  const content = await getDocContent(slug);
  if (!content) {
    notFound();
  }

  const Content = content.default;
  return <Content />;
}
