import Link from 'next/link';
import { CodeBlock } from '@/components/CodeBlock';

const rtsCode = `import { Serialize } from "serde";

type Book = {
  title: string,
  author: string,
  rating: f64,
} derives Serialize

function main() {
  const books: Array<Book> = [
    { title: "Dune", author: "Herbert", rating: 4.7 },
    { title: "Neuromancer", author: "Gibson", rating: 4.5 },
  ];

  const top = books.filter(b => b.rating > 4.6);
  console.log(JSON.stringify(top));
}`;

const rsCode = `use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
struct Book {
    pub title: String,
    pub author: String,
    pub rating: f64,
}

fn main() {
    let books: Vec<Book> = vec![
        Book { title: "Dune".to_string(), author: "Herbert".to_string(), rating: 4.7 },
        Book { title: "Neuromancer".to_string(), author: "Gibson".to_string(), rating: 4.5 },
    ];

    let top: Vec<Book> = books.iter()
        .filter(|b| b.rating > 4.6).cloned().collect();
    println!("{}", serde_json::to_string(&top).unwrap());
}`;

const installCode = `cargo install rustscript  # also installs rsc alias
rustscript init my-app --template web-server
cd my-app
rustscript run`;

const stats = [
  { value: '2,600+', label: 'Tests' },
  { value: '330+', label: 'Built-in Methods' },
  { value: '11', label: 'Crates' },
  { value: '195', label: 'Conformance Tests' },
];

const features = [
  {
    title: 'Familiar Syntax',
    bullets: [
      'Every TypeScript pattern: classes, generics, async/await, destructuring',
      '330+ built-in methods: map, filter, reduce, find, forEach, and more',
      'Template literals, optional chaining, nullish coalescing, spread',
      'String unions, type aliases, interfaces, discriminated unions',
    ],
  },
  {
    title: 'Rust Performance',
    bullets: [
      '3MB native binaries — no V8, no garbage collector',
      'No runtime overhead — compiles to idiomatic Rust',
      'Generated code is human-readable and inspectable',
      'Eject to pure Rust at any time — no lock-in',
    ],
  },
  {
    title: 'Full Crate Ecosystem',
    bullets: [
      'import { Router } from "axum" — any Rust crate, TS import syntax',
      'Dependencies auto-detected from imports — no Cargo.toml editing',
      'derives keyword for proc macros: Serialize, Deserialize, Parser',
      'axum, serde, tokio, clap, reqwest, sqlx — they all just work',
    ],
  },
  {
    title: 'Zero Memory Management',
    bullets: [
      'No lifetimes, no borrowing annotations, no ownership errors',
      'Compiler infers ownership and inserts clones for correctness',
      'Tier 2 borrow inference eliminates unnecessary allocations',
      'Async/await with tokio — just works, no runtime configuration',
    ],
  },
  {
    title: 'Production Tooling',
    bullets: [
      'VS Code extension with real language server (LSP)',
      'Type-aware hover: signatures, doc comments, inferred types',
      'Closure parameter inference, generic substitution, field resolution',
      'Live diagnostics, code formatting, watch mode, project templates',
    ],
  },
  {
    title: 'Friendly Error Messages',
    bullets: [
      'Errors point to your .rts source lines, not generated Rust',
      'Dense source maps with O(1) line lookup',
      'rustc JSON diagnostics parsed and re-rendered with RustScript types',
      '9 enrichment patterns: type translation, borrow hints, lifetime suggestions',
    ],
  },
  {
    title: 'Type Generator',
    bullets: [
      'rsc types emits .d.ts files from RustScript source',
      'One file per module — TypeScript-native output',
      'Share types between RustScript backend and TS frontend',
      'rsc build --emit-types for CI pipelines — perfect for Tauri apps',
    ],
  },
  {
    title: 'Eject Anytime',
    bullets: [
      'Generated Rust compiles with standard rustc',
      'Uses standard crates, follows Rust conventions',
      'No custom runtime, no code generation magic',
      'Walk away to pure Rust whenever you want',
    ],
  },
];

const examples = [
  {
    name: 'Tauri Desktop App',
    description: 'Notes app with RustScript backend, React frontend, shared types, and @command decorators.',
    href: 'https://github.com/jmfirth/rustscript/tree/main/examples/tauri_notes',
  },
  {
    name: 'REST API',
    description: 'Book catalog with axum + serde. Typed JSON responses, filter/map/reduce pipelines.',
    href: 'https://github.com/jmfirth/rustscript/tree/main/examples/json_api',
  },
  {
    name: 'HTTP Client',
    description: 'Async HTTP client with reqwest. Parallel fetches with Promise.all, typed responses.',
    href: 'https://github.com/jmfirth/rustscript/tree/main/examples/http_client',
  },
  {
    name: 'CLI Tool',
    description: 'Task manager with clap. Command dispatch, search, filtering, and formatted output.',
    href: 'https://github.com/jmfirth/rustscript/tree/main/examples/cli_tool',
  },
];

export default function HomePage() {
  return (
    <div>
      {/* Hero */}
      <section className="py-20 sm:py-32">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="max-w-3xl">
            <h1 className="text-4xl sm:text-5xl lg:text-6xl font-bold tracking-tight">
              Ship <span className="text-[var(--color-accent-secondary)]">Rust</span>.{' '}
              Write <span className="text-[var(--color-accent)]">TypeScript</span>.
            </h1>
            <p className="mt-6 text-lg sm:text-xl text-[var(--color-text-secondary)] leading-relaxed">
              A complete development ecosystem for building native applications
              with the TypeScript syntax you already know. 3MB binaries. No V8.
              No garbage collector.
            </p>
            <div className="mt-8 flex flex-wrap gap-4">
              <Link
                href="/playground"
                className="inline-flex items-center px-6 py-3 rounded-lg bg-[var(--color-accent)] text-white font-medium hover:opacity-90 transition-opacity"
              >
                Try the Playground &rarr;
              </Link>
              <Link
                href="/docs"
                className="inline-flex items-center px-6 py-3 rounded-lg border border-[var(--color-border)] font-medium hover:bg-[var(--color-bg-secondary)] transition-colors"
              >
                Read the Docs &rarr;
              </Link>
            </div>
          </div>

          {/* Code comparison */}
          <div className="mt-16 grid md:grid-cols-2 gap-4">
            <CodeBlock code={rtsCode} language="typescript" filename="app.rts" />
            <CodeBlock code={rsCode} language="rust" filename="app.rs (generated)" />
          </div>
        </div>
      </section>

      {/* Stats bar */}
      <section className="border-y border-[var(--color-border)] bg-[var(--color-bg-secondary)]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
          <div className="grid grid-cols-2 md:grid-cols-4 gap-8 text-center">
            {stats.map((stat) => (
              <div key={stat.label}>
                <div className="text-3xl font-bold text-[var(--color-accent-secondary)]">
                  {stat.value}
                </div>
                <div className="text-sm text-[var(--color-text-secondary)] mt-1">
                  {stat.label}
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Getting Started */}
      <section className="py-20">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="max-w-2xl mx-auto text-center">
            <h2 className="text-3xl font-bold mb-8">Get Started in 30 Seconds</h2>
            <div className="text-left">
              <CodeBlock code={installCode} language="bash" filename="terminal" />
            </div>
          </div>
        </div>
      </section>

      {/* Crate docs */}
      <section className="py-20 bg-[var(--color-bg-secondary)]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 text-center">
          <h2 className="text-3xl font-bold mb-4">
            Every Rust crate, in TypeScript syntax
          </h2>
          <p className="text-[var(--color-text-secondary)] mb-8 max-w-2xl mx-auto">
            The entire Rust ecosystem already speaks your language. Browse any crate,
            any version, with its public API translated to RustScript syntax on demand.
          </p>
          <div className="flex flex-wrap justify-center gap-3 mb-8">
            {['axum', 'serde', 'tokio', 'clap', 'reqwest', 'sqlx'].map((crate_name) => (
              <Link
                key={crate_name}
                href={`/crates?name=${crate_name}&version=latest`}
                className="px-4 py-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] text-sm font-mono hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] transition-colors"
              >
                {crate_name}
              </Link>
            ))}
          </div>
          <Link
            href="/crates"
            className="inline-flex items-center px-8 py-3 rounded-lg bg-[var(--color-accent)] text-white font-medium hover:opacity-90 transition-opacity"
          >
            Browse All Crates &rarr;
          </Link>
        </div>
      </section>

      {/* Feature grid */}
      <section className="py-20 bg-[var(--color-bg-secondary)]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <h2 className="text-3xl font-bold text-center mb-4">
            Everything you need to ship Rust
          </h2>
          <p className="text-center text-[var(--color-text-secondary)] mb-12 max-w-2xl mx-auto">
            Not a prototype. A complete compiler with full TypeScript syntax coverage,
            a standard library, production tooling, and zero known conformance gaps.
          </p>
          <div className="grid md:grid-cols-2 gap-6">
            {features.map((feature) => (
              <div
                key={feature.title}
                className="p-6 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] hover:border-[var(--color-accent)] transition-colors"
              >
                <h3 className="text-lg font-semibold mb-3">{feature.title}</h3>
                <ul className="space-y-1.5">
                  {feature.bullets.map((bullet) => (
                    <li key={bullet} className="text-sm text-[var(--color-text-secondary)] leading-relaxed flex gap-2">
                      <span className="text-[var(--color-accent-secondary)] mt-0.5 shrink-0">&bull;</span>
                      <span>{bullet}</span>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Playground CTA */}
      <section className="py-16 bg-[var(--color-bg-secondary)]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 text-center">
          <h2 className="text-2xl font-bold mb-3">
            Try it in your browser
          </h2>
          <p className="text-[var(--color-text-secondary)] mb-6 max-w-lg mx-auto">
            Live compilation, real diagnostics, and TypeScript-grade hover tooltips &mdash;
            all running client-side via WebAssembly. No server required.
          </p>
          <Link
            href="/playground"
            className="inline-flex items-center px-8 py-3 rounded-lg bg-[var(--color-accent)] text-white font-medium hover:opacity-90 transition-opacity"
          >
            Open Playground &rarr;
          </Link>
        </div>
      </section>

      {/* Examples */}
      <section className="py-20 bg-[var(--color-bg-secondary)]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <h2 className="text-3xl font-bold text-center mb-4">
            Real-world examples
          </h2>
          <p className="text-center text-[var(--color-text-secondary)] mb-12 max-w-2xl mx-auto">
            Every example compiles to a native binary. No scaffolding, no boilerplate.
          </p>
          <div className="grid sm:grid-cols-2 lg:grid-cols-4 gap-4">
            {examples.map((example) => (
              <a
                key={example.name}
                href={example.href}
                target="_blank"
                rel="noopener noreferrer"
                className="p-6 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] hover:border-[var(--color-accent)] transition-colors group"
              >
                <h3 className="font-semibold mb-2 group-hover:text-[var(--color-accent)] transition-colors">{example.name}</h3>
                <p className="text-sm text-[var(--color-text-secondary)] leading-relaxed">
                  {example.description}
                </p>
              </a>
            ))}
          </div>
        </div>
      </section>

    </div>
  );
}
