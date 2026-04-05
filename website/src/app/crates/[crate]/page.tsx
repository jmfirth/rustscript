import { CrateDocsViewer } from './CrateDocsViewer';

const knownCrates = ['axum', 'serde', 'tokio', 'clap', 'reqwest', 'sqlx'];

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
