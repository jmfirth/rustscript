'use client';

import dynamic from 'next/dynamic';

const PlaygroundEditor = dynamic(
  () => import('@/components/PlaygroundEditor').then((mod) => mod.PlaygroundEditor),
  {
    ssr: false,
    loading: () => (
      <div className="flex-1 flex items-center justify-center text-[var(--color-text-secondary)]">
        Loading playground...
      </div>
    ),
  }
);

export default function PlaygroundPage() {
  return (
    <div className="flex flex-col" style={{ height: 'calc(100vh - 4rem)' }}>
      <PlaygroundEditor />
    </div>
  );
}
