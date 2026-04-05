import Link from 'next/link';

const sidebarItems = [
  {
    title: 'Getting Started',
    items: [
      { title: 'Installation', href: '/docs/getting-started/installation' },
    ],
  },
  {
    title: 'Guide',
    items: [
      { title: 'Overview', href: '/docs' },
    ],
  },
];

export default function DocsLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
      <div className="flex gap-8">
        <aside className="hidden md:block w-64 shrink-0">
          <nav className="sticky top-24 space-y-6">
            {sidebarItems.map((section) => (
              <div key={section.title}>
                <h4 className="font-semibold text-sm uppercase tracking-wider text-[var(--color-text-secondary)] mb-2">
                  {section.title}
                </h4>
                <ul className="space-y-1">
                  {section.items.map((item) => (
                    <li key={item.href}>
                      <Link
                        href={item.href}
                        className="block px-3 py-1.5 text-sm rounded-md hover:bg-[var(--color-bg-secondary)] text-[var(--color-text-secondary)] hover:text-[var(--color-text)] transition-colors"
                      >
                        {item.title}
                      </Link>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </nav>
        </aside>
        <div className="flex-1 min-w-0">
          <article className="prose max-w-none">{children}</article>
        </div>
      </div>
    </div>
  );
}
