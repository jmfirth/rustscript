/**
 * MDX content registry.
 *
 * Maps slug paths to lazily imported MDX modules. For static export,
 * every doc page must be registered here so `generateStaticParams`
 * can enumerate them at build time.
 */

import type { ComponentType } from 'react';

export interface DocPage {
  slug: string[];
  title: string;
  description: string;
}

export const docPages: DocPage[] = [
  {
    slug: ['getting-started', 'installation'],
    title: 'Installation',
    description: 'Install the RustScript compiler and create your first project.',
  },
];

/**
 * Load MDX content by slug. Returns null if not found.
 */
export async function getDocContent(
  slug: string[]
): Promise<{ default: ComponentType } | null> {
  const path = slug.join('/');

  const modules: Record<string, () => Promise<{ default: ComponentType }>> = {
    'getting-started/installation': () =>
      import('@/content/docs/getting-started/installation.mdx'),
  };

  const loader = modules[path];
  if (!loader) return null;

  return loader();
}
