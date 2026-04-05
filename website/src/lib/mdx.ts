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
  // Getting Started
  {
    slug: ['getting-started', 'installation'],
    title: 'Installation',
    description: 'Install the RustScript compiler and create your first project.',
  },
  {
    slug: ['getting-started', 'hello-world'],
    title: 'Hello World',
    description: 'Write your first RustScript program and understand what happens under the hood.',
  },
  {
    slug: ['getting-started', 'project-structure'],
    title: 'Project Structure',
    description: 'Understand the layout of a RustScript project and the compilation pipeline.',
  },
  // Language
  {
    slug: ['language', 'types'],
    title: 'Types',
    description: 'Primitives, collections, tuples, nullable types, and numeric types in RustScript.',
  },
  {
    slug: ['language', 'functions'],
    title: 'Functions',
    description: 'Function declarations, async functions, arrow functions, closures, and generics.',
  },
  {
    slug: ['language', 'variables'],
    title: 'Variables',
    description: 'const vs let vs var, destructuring, spread operator, and operators.',
  },
  {
    slug: ['language', 'control-flow'],
    title: 'Control Flow',
    description: 'if/else, for-of, for-in, classic for, while, do-while, switch, and labeled loops.',
  },
  {
    slug: ['language', 'classes'],
    title: 'Classes',
    description: 'Class declarations, constructors, inheritance with extends, and Rust mapping.',
  },
  {
    slug: ['language', 'type-definitions'],
    title: 'Type Definitions',
    description: 'Struct types, derives, string enums, data enums, optional fields, and type aliases.',
  },
  {
    slug: ['language', 'modules'],
    title: 'Modules',
    description: 'Import/export, importing from Rust crates, submodule imports, and crate detection.',
  },
  {
    slug: ['language', 'error-handling'],
    title: 'Error Handling',
    description: 'try/catch, throw, Error object, and how it maps to Rust Result and panic.',
  },
  // Builtins
  {
    slug: ['builtins', 'string'],
    title: 'String',
    description: 'All string methods with examples and Rust equivalents.',
  },
  {
    slug: ['builtins', 'array'],
    title: 'Array',
    description: 'All array methods: map, filter, reduce, find, sort, and more.',
  },
  {
    slug: ['builtins', 'map-set'],
    title: 'Map & Set',
    description: 'Map and Set construction, methods, and iteration patterns.',
  },
  {
    slug: ['builtins', 'math'],
    title: 'Math',
    description: 'Math static methods: rounding, powers, trigonometry, constants.',
  },
  {
    slug: ['builtins', 'date'],
    title: 'Date',
    description: 'Date construction, getters, setters, and formatting methods.',
  },
  {
    slug: ['builtins', 'json'],
    title: 'JSON',
    description: 'JSON.stringify, JSON.parse, and serde integration.',
  },
  {
    slug: ['builtins', 'console'],
    title: 'Console',
    description: 'console.log, error, warn, debug, table, timers, counters, and assertions.',
  },
  {
    slug: ['builtins', 'promise'],
    title: 'Promise & Async',
    description: 'Promise.all, Promise.allSettled, async/await, timers, and the tokio runtime.',
  },
  // Ecosystem
  {
    slug: ['ecosystem', 'importing-crates'],
    title: 'Importing Crates',
    description: 'How import from "crate" works, auto-dependency detection, and Cargo.toml generation.',
  },
  {
    slug: ['ecosystem', 'derives'],
    title: 'Derives',
    description: 'The derives keyword, common derives, and when to use them.',
  },
  {
    slug: ['ecosystem', 'rust-blocks'],
    title: 'Rust Blocks',
    description: 'The rust { } escape hatch for writing raw Rust inside RustScript.',
  },
  {
    slug: ['ecosystem', 'type-mapping'],
    title: 'Type Mapping',
    description: 'Complete reference: TypeScript types to Rust types, reading Rust crate docs.',
  },
  // Reference
  {
    slug: ['reference', 'cli-commands'],
    title: 'CLI Commands',
    description: 'All rsc commands: init, build, run, check, fmt. Flags and options.',
  },
  {
    slug: ['reference', 'cheatsheet'],
    title: 'Cheatsheet',
    description: 'Dense single-page reference for RustScript syntax, types, and builtins.',
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
    // Getting Started
    'getting-started/installation': () =>
      import('@/content/docs/getting-started/installation.mdx'),
    'getting-started/hello-world': () =>
      import('@/content/docs/getting-started/hello-world.mdx'),
    'getting-started/project-structure': () =>
      import('@/content/docs/getting-started/project-structure.mdx'),
    // Language
    'language/types': () =>
      import('@/content/docs/language/types.mdx'),
    'language/functions': () =>
      import('@/content/docs/language/functions.mdx'),
    'language/variables': () =>
      import('@/content/docs/language/variables.mdx'),
    'language/control-flow': () =>
      import('@/content/docs/language/control-flow.mdx'),
    'language/classes': () =>
      import('@/content/docs/language/classes.mdx'),
    'language/type-definitions': () =>
      import('@/content/docs/language/type-definitions.mdx'),
    'language/modules': () =>
      import('@/content/docs/language/modules.mdx'),
    'language/error-handling': () =>
      import('@/content/docs/language/error-handling.mdx'),
    // Builtins
    'builtins/string': () =>
      import('@/content/docs/builtins/string.mdx'),
    'builtins/array': () =>
      import('@/content/docs/builtins/array.mdx'),
    'builtins/map-set': () =>
      import('@/content/docs/builtins/map-set.mdx'),
    'builtins/math': () =>
      import('@/content/docs/builtins/math.mdx'),
    'builtins/date': () =>
      import('@/content/docs/builtins/date.mdx'),
    'builtins/json': () =>
      import('@/content/docs/builtins/json.mdx'),
    'builtins/console': () =>
      import('@/content/docs/builtins/console.mdx'),
    'builtins/promise': () =>
      import('@/content/docs/builtins/promise.mdx'),
    // Ecosystem
    'ecosystem/importing-crates': () =>
      import('@/content/docs/ecosystem/importing-crates.mdx'),
    'ecosystem/derives': () =>
      import('@/content/docs/ecosystem/derives.mdx'),
    'ecosystem/rust-blocks': () =>
      import('@/content/docs/ecosystem/rust-blocks.mdx'),
    'ecosystem/type-mapping': () =>
      import('@/content/docs/ecosystem/type-mapping.mdx'),
    // Reference
    'reference/cli-commands': () =>
      import('@/content/docs/reference/cli-commands.mdx'),
    'reference/cheatsheet': () =>
      import('@/content/docs/reference/cheatsheet.mdx'),
  };

  const loader = modules[path];
  if (!loader) return null;

  return loader();
}
