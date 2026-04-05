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

const installCode = `cargo install rsc
rsc init my-app --template web-server
cd my-app
rsc run`;

const stats = [
  { value: '2,600+', label: 'Tests' },
  { value: '330+', label: 'Builtins' },
  { value: '11', label: 'Crates' },
  { value: '0', label: 'Conformance Gaps' },
];

const features = [
  {
    title: 'Familiar Syntax',
    description: 'Every TypeScript pattern you know. Classes, generics, async/await, destructuring, arrow functions, template literals, optional chaining, nullish coalescing. 330+ standard library methods — map, filter, reduce, find, forEach, and everything else.',
  },
  {
    title: 'Rust Performance',
    description: '3MB native binaries. No V8. No garbage collector. No runtime overhead. Your code compiles to idiomatic, human-readable Rust that you can inspect, debug, and eject to at any time.',
  },
  {
    title: 'Full Crate Ecosystem',
    description: 'import { Router } from "axum". Any Rust crate, TypeScript import syntax. Dependencies auto-detected from your imports — no Cargo.toml editing. serde, axum, tokio, clap, reqwest, sqlx — they all just work.',
  },
  {
    title: 'Zero Memory Management',
    description: 'No lifetimes. No borrowing annotations. No ownership errors. The compiler infers ownership, inserts clones for correctness, and applies Tier 2 borrow inference to eliminate unnecessary allocations — automatically.',
  },
  {
    title: 'Production Tooling',
    description: 'VS Code extension with a real language server. Type-aware hover showing signatures, doc comments, and inferred types — including closure parameters and generic substitution. Red squiggles on errors. Code formatting. Watch mode. Project templates.',
  },
  {
    title: 'Friendly Error Messages',
    description: 'Errors point to your .rts source lines, not generated Rust. Tier 2 error enrichment parses rustc JSON diagnostics, maps them through dense source maps, and re-renders with RustScript type names and actionable suggestions.',
  },
  {
    title: 'Type Generator',
    description: 'rsc types generates .d.ts files from your RustScript types — one file per module, TypeScript-native output. Share types between your RustScript backend and TypeScript frontend. Perfect for Tauri desktop apps.',
  },
  {
    title: 'Eject Anytime',
    description: 'The generated Rust is yours. It compiles with standard rustc, uses standard crates, follows Rust conventions. No runtime. No lock-in. Walk away to pure Rust whenever you want — the code is readable and idiomatic.',
  },
];

const examples = [
  {
    name: 'REST API',
    description: 'Book catalog with axum + serde. 8 endpoints, typed JSON responses, filter/map/reduce.',
    href: 'https://github.com/user/rsc/tree/main/examples/json_api',
  },
  {
    name: 'HTTP Client',
    description: 'Async HTTP client with reqwest. Fetch from JSONPlaceholder, parse JSON, process data.',
    href: 'https://github.com/user/rsc/tree/main/examples/http_client',
  },
  {
    name: 'CLI Tool',
    description: 'Task manager with command dispatch, search, filtering, and formatted output.',
    href: 'https://github.com/user/rsc/tree/main/examples/cli_tool',
  },
  {
    name: 'Tauri Desktop App',
    description: 'Notes app with RustScript backend, React frontend, and shared types via rsc types.',
    href: 'https://github.com/user/rsc/tree/main/examples/tauri_notes',
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
              Write TypeScript.{' '}
              <span className="text-[var(--color-accent-secondary)]">Ship Rust.</span>
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

      {/* Feature grid */}
      <section className="py-20">
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
                className="p-6 rounded-lg border border-[var(--color-border)] hover:border-[var(--color-accent)] transition-colors"
              >
                <h3 className="text-lg font-semibold mb-2">{feature.title}</h3>
                <p className="text-sm text-[var(--color-text-secondary)] leading-relaxed">
                  {feature.description}
                </p>
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
      <section className="py-20">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <h2 className="text-3xl font-bold text-center mb-4">
            Real-world examples
          </h2>
          <p className="text-center text-[var(--color-text-secondary)] mb-12 max-w-2xl mx-auto">
            Every example compiles to a native binary. No scaffolding, no boilerplate.
          </p>
          <div className="grid sm:grid-cols-2 lg:grid-cols-4 gap-4">
            {examples.map((example) => (
              <div
                key={example.name}
                className="p-6 rounded-lg border border-[var(--color-border)]"
              >
                <h3 className="font-semibold mb-2">{example.name}</h3>
                <p className="text-sm text-[var(--color-text-secondary)] leading-relaxed">
                  {example.description}
                </p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Crate docs CTA */}
      <section className="py-16 bg-[var(--color-bg-secondary)]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 text-center">
          <h2 className="text-2xl font-bold mb-3">
            Browse crate APIs in RustScript syntax
          </h2>
          <p className="text-[var(--color-text-secondary)] mb-6 max-w-lg mx-auto">
            Look up any Rust crate and see its public API translated to familiar
            TypeScript-style signatures. Powered by rustdoc JSON and WebAssembly.
          </p>
          <Link
            href="/crates"
            className="inline-flex items-center px-8 py-3 rounded-lg border border-[var(--color-border)] font-medium hover:bg-[var(--color-bg)] transition-colors"
          >
            Explore Crates &rarr;
          </Link>
        </div>
      </section>

      {/* Getting Started */}
      <section className="py-20">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="max-w-2xl mx-auto text-center">
            <h2 className="text-3xl font-bold mb-8">Get Started</h2>
            <div className="text-left">
              <CodeBlock code={installCode} language="bash" filename="terminal" />
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
