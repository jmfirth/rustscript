export interface PlaygroundExample {
  id: string;
  label: string;
  rts: string;
  rs: string;
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
    rs: `fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}

fn main() {
    let message: String = greet("world".to_string());
    println!("{}", message);

    let names: Vec<String> = vec![
        "Alice".to_string(),
        "Bob".to_string(),
        "Charlie".to_string(),
    ];
    for name in &names {
        println!("{}", greet(name.clone()));
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
    rs: `use serde::Serialize;

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
        Book { title: "Foundation".to_string(), author: "Asimov".to_string(), rating: 4.8 },
    ];

    let top: Vec<Book> = books.iter().filter(|b| b.rating > 4.6).cloned().collect();
    println!("{}", serde_json::to_string(&top).unwrap());

    let titles: Vec<String> = books.iter().map(|b| b.title.clone()).collect();
    println!("{}", titles.join(", "));
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
    rs: `async fn fetch_user(id: i32) -> String {
    format!("User {}", id)
}

async fn fetch_score(id: i32) -> f64 {
    id as f64 * 10.5
}

#[tokio::main]
async fn main() {
    let user: String = fetch_user(1).await;
    println!("{}", user);

    let (a, b) = tokio::join!(
        fetch_user(2),
        fetch_user(3),
    );
    println!("Got: {} and {}", a, b);

    let score: f64 = fetch_score(1).await;
    println!("Score: {}", score);
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
    rs: `fn identity<T>(value: T) -> T {
    value
}

fn pair<A, B>(a: A, b: B) -> (A, B) {
    (a, b)
}

fn longest(a: String, b: String) -> String {
    if a.len() as i64 > b.len() as i64 {
        return a;
    }
    b
}

fn main() {
    let s: String = identity::<String>("hello".to_string());
    let n: i32 = identity::<i32>(42);
    println!("{}, {}", s, n);

    let p: (String, i32) = pair::<String, i32>("age".to_string(), 30);
    println!("{}: {}", p.0, p.1);

    let result: String = longest("short".to_string(), "much longer".to_string());
    println!("{}", result);
}`,
  },
];
