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

    let top: Vec<Book> = books.iter().filter(|b| b.rating > 4.6).cloned().collect();
    println!("{}", serde_json::to_string(&top).unwrap());
}`;

const installCode = `cargo install rsc
rsc init my-app
cd my-app
rsc run`;

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
              3MB binaries. No V8. No garbage collector. Your TypeScript skills,
              Rust&apos;s performance.
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

      {/* Value Props */}
      <section className="py-20 bg-[var(--color-bg-secondary)]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="grid md:grid-cols-3 gap-8">
            <div>
              <h3 className="text-xl font-semibold mb-3">Familiar Syntax</h3>
              <p className="text-[var(--color-text-secondary)] leading-relaxed">
                If you know TypeScript, you already know RustScript. Same syntax,
                same patterns, same standard library methods.
              </p>
            </div>
            <div>
              <h3 className="text-xl font-semibold mb-3">Rust Performance</h3>
              <p className="text-[var(--color-text-secondary)] leading-relaxed">
                Your code compiles to idiomatic Rust. No runtime overhead, no
                garbage collector, no V8. Just native speed.
              </p>
            </div>
            <div>
              <h3 className="text-xl font-semibold mb-3">Full Ecosystem</h3>
              <p className="text-[var(--color-text-secondary)] leading-relaxed">
                Import any Rust crate with TypeScript import syntax. serde, axum,
                tokio, clap &mdash; they all just work.
              </p>
            </div>
          </div>
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
