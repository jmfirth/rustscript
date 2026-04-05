import Link from 'next/link';

const sidebarItems = [
  {
    title: 'Getting Started',
    items: [
      { title: 'Installation', href: '/docs/getting-started/installation' },
      { title: 'Hello World', href: '/docs/getting-started/hello-world' },
      { title: 'Project Structure', href: '/docs/getting-started/project-structure' },
    ],
  },
  {
    title: 'Language',
    items: [
      { title: 'Types', href: '/docs/language/types' },
      { title: 'Variables', href: '/docs/language/variables' },
      { title: 'Functions', href: '/docs/language/functions' },
      { title: 'Control Flow', href: '/docs/language/control-flow' },
      { title: 'Classes', href: '/docs/language/classes' },
      { title: 'Type Definitions', href: '/docs/language/type-definitions' },
      { title: 'Modules', href: '/docs/language/modules' },
      { title: 'Error Handling', href: '/docs/language/error-handling' },
    ],
  },
  {
    title: 'Builtins',
    items: [
      { title: 'String', href: '/docs/builtins/string' },
      { title: 'Array', href: '/docs/builtins/array' },
      { title: 'Map & Set', href: '/docs/builtins/map-set' },
      { title: 'Math', href: '/docs/builtins/math' },
      { title: 'Date', href: '/docs/builtins/date' },
      { title: 'JSON', href: '/docs/builtins/json' },
      { title: 'Console', href: '/docs/builtins/console' },
      { title: 'Promise & Async', href: '/docs/builtins/promise' },
    ],
  },
  {
    title: 'Ecosystem',
    items: [
      { title: 'Importing Crates', href: '/docs/ecosystem/importing-crates' },
      { title: 'Derives', href: '/docs/ecosystem/derives' },
      { title: 'Rust Blocks', href: '/docs/ecosystem/rust-blocks' },
      { title: 'Type Mapping', href: '/docs/ecosystem/type-mapping' },
    ],
  },
  {
    title: 'Reference',
    items: [
      { title: 'CLI Commands', href: '/docs/reference/cli-commands' },
      { title: 'Cheatsheet', href: '/docs/reference/cheatsheet' },
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
