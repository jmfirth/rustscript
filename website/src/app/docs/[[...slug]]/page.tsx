import { notFound } from 'next/navigation';
import { docPages, getDocContent } from '@/lib/mdx';

export function generateStaticParams() {
  return docPages.map((page) => ({
    slug: page.slug,
  }));
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ slug?: string[] }>;
}) {
  const { slug } = await params;
  if (!slug) return {};

  const page = docPages.find(
    (p) => p.slug.join('/') === slug.join('/')
  );

  return {
    title: page ? `${page.title} - RustScript Docs` : 'RustScript Docs',
    description: page?.description,
  };
}

export default async function DocPage({
  params,
}: {
  params: Promise<{ slug?: string[] }>;
}) {
  const { slug } = await params;

  // If no slug, this conflicts with docs/page.tsx index.
  // The catch-all only handles sub-paths.
  if (!slug || slug.length === 0) {
    notFound();
  }

  const content = await getDocContent(slug);
  if (!content) {
    notFound();
  }

  const Content = content.default;
  return <Content />;
}
