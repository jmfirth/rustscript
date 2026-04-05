import { CrateDocsViewer } from './CrateDocsViewer';

// Pre-generated routes for static export. Any crate not in this list
// needs to be added here for `output: 'export'` to generate its page.
// In a future version, this could move to SSR or ISR to support arbitrary crates.
const knownCrates = [
  'axum', 'serde', 'tokio', 'clap', 'reqwest', 'sqlx',
  'thiserror', 'anyhow', 'regex', 'serde_json', 'rand',
  'tracing', 'hyper', 'tower', 'bytes', 'futures',
];

export function generateStaticParams() {
  return knownCrates.map((name) => ({ crate: name }));
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ crate: string }>;
}) {
  const { crate: crateName } = await params;
  return {
    title: `${crateName} - RustScript Crate Docs`,
  };
}

export default async function CratePage({
  params,
}: {
  params: Promise<{ crate: string }>;
}) {
  const { crate: crateName } = await params;
  return <CrateDocsViewer crateName={crateName} />;
}
