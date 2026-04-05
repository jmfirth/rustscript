export interface PlaygroundExample {
  id: string;
  label: string;
  rts: string;
}

export const examples: PlaygroundExample[] = [
  {
    id: 'hello-world',
    label: 'Hello World',
    rts: `function greet(name: string): string {
  return \`Hello, \${name}!\`;
}

function main() {
  const message = greet("world");
  console.log(message);

  const names: Array<string> = ["Alice", "Bob", "Charlie"];
  for (const name of names) {
    console.log(greet(name));
  }
}`,
  },
  {
    id: 'book-api',
    label: 'Book API',
    rts: `import { Serialize } from "serde";

type Book = {
  title: string,
  author: string,
  rating: f64,
} derives Serialize

function main() {
  const books: Array<Book> = [
    { title: "Dune", author: "Herbert", rating: 4.7 },
    { title: "Neuromancer", author: "Gibson", rating: 4.5 },
    { title: "Foundation", author: "Asimov", rating: 4.8 },
  ];

  const top = books.filter(b => b.rating > 4.6);
  console.log(JSON.stringify(top));

  const titles = books.map(b => b.title);
  console.log(titles.join(", "));
}`,
  },
  {
    id: 'async-await',
    label: 'Async / Await',
    rts: `async function fetchUser(id: i32): string {
  return \`User \${id}\`;
}

async function fetchScore(id: i32): f64 {
  return id as f64 * 10.5;
}

async function main() {
  const user = await fetchUser(1);
  console.log(user);

  const [a, b] = await Promise.all([
    fetchUser(2),
    fetchUser(3),
  ]);
  console.log(\`Got: \${a} and \${b}\`);

  const score = await fetchScore(1);
  console.log(\`Score: \${score}\`);
}`,
  },
  {
    id: 'generics',
    label: 'Generics',
    rts: `function identity<T>(value: T): T {
  return value;
}

function pair<A, B>(a: A, b: B): [A, B] {
  return [a, b];
}

function longest(a: string, b: string): string {
  if (a.length > b.length) {
    return a;
  }
  return b;
}

function main() {
  const s = identity<string>("hello");
  const n = identity<i32>(42);
  console.log(\`\${s}, \${n}\`);

  const p = pair<string, i32>("age", 30);
  console.log(\`\${p[0]}: \${p[1]}\`);

  const result = longest("short", "much longer");
  console.log(result);
}`,
  },
];
