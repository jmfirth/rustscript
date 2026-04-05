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
  {
    id: 'tauri-backend',
    label: 'Tauri Backend',
    rts: `import { command } from "tauri";
import { Serialize, Deserialize } from "serde";

/** A note in the app */
type Note = {
  id: u32,
  title: string,
  content: string,
  pinned: bool,
} derives Serialize, Deserialize

/** Get all notes */
@command
function get_notes(): Array<Note> {
  const notes: Array<Note> = [
    { id: 1, title: "Welcome", content: "Hello from RustScript!", pinned: true },
    { id: 2, title: "Getting Started", content: "Edit main.rts to add commands.", pinned: false },
  ];
  return notes;
}

/** Search notes by title */
@command
function search_notes(query: string): Array<Note> {
  const notes = get_notes();
  const lower = query.toLowerCase();
  return notes.filter(n => n.title.toLowerCase().includes(lower));
}

function main() {
  rust {
    tauri::Builder::default()
      .invoke_handler(tauri::generate_handler![get_notes, search_notes])
      .run(tauri::generate_context!())
      .expect("error running tauri app");
  }
}`,
  },
  {
    id: 'error-handling',
    label: 'Error Handling',
    rts: `/** Validate age — throws on invalid input */
function validateAge(age: i32): i32 throws string {
  if (age < 0) {
    throw "age cannot be negative";
  }
  if (age > 150) {
    throw "age seems unrealistic";
  }
  return age;
}

/** Look up a user — might not exist */
function findUser(id: i32): string | null {
  const users = new Map<i32, string>();
  users.set(1, "Alice");
  users.set(2, "Bob");
  return users.get(id);
}

function main() {
  // try/catch maps to Result matching
  try {
    const age = validateAge(25);
    console.log(\`Valid age: \${age}\`);
  } catch (e) {
    console.log(\`Error: \${e}\`);
  }

  try {
    const bad = validateAge(-5);
    console.log(\`Age: \${bad}\`);
  } catch (e) {
    console.log(\`Caught: \${e}\`);
  }

  // T | null maps to Option<T>
  const user = findUser(1);
  if (user != null) {
    console.log(\`Found: \${user}\`);
  }

  // Null coalescing
  const name = findUser(99) ?? "anonymous";
  console.log(name);
}`,
  },
  {
    id: 'classes-interfaces',
    label: 'Classes & Interfaces',
    rts: `interface Describable {
  describe(): string;
}

class Animal {
  name: string;
  sound: string;

  constructor(name: string, sound: string) {
    this.name = name;
    this.sound = sound;
  }

  speak(): string {
    return \`\${this.name} says \${this.sound}\`;
  }
}

class Dog extends Animal implements Describable {
  breed: string;

  constructor(name: string, breed: string) {
    super(name, "woof");
    this.breed = breed;
  }

  describe(): string {
    return \`\${this.name} is a \${this.breed}\`;
  }
}

function main() {
  const dog = new Dog("Rex", "German Shepherd");
  console.log(dog.speak());
  console.log(dog.describe());
}`,
  },
  {
    id: 'iterators',
    label: 'Iterator Pipeline',
    rts: `import { Serialize } from "serde";

type Employee = {
  name: string,
  department: string,
  salary: f64,
} derives Serialize

function main() {
  const team: Array<Employee> = [
    { name: "Alice", department: "Engineering", salary: 130000.0 },
    { name: "Bob", department: "Design", salary: 95000.0 },
    { name: "Charlie", department: "Engineering", salary: 145000.0 },
    { name: "Diana", department: "Engineering", salary: 120000.0 },
    { name: "Eve", department: "Design", salary: 105000.0 },
  ];

  // Filter → map → collect
  const engineers = team
    .filter(e => e.department == "Engineering")
    .map(e => e.name);
  console.log(\`Engineers: \${engineers.join(", ")}\`);

  // Reduce to sum
  const totalSalary = team
    .map(e => e.salary)
    .reduce((sum, s) => sum + s, 0.0);
  console.log(\`Total payroll: \${totalSalary}\`);

  // Find first match
  const senior = team.find(e => e.salary > 140000.0);
  if (senior != null) {
    console.log(\`Top earner: \${senior.name}\`);
  }

  // Check conditions
  const allPaid = team.every(e => e.salary > 50000.0);
  const hasDesigner = team.some(e => e.department == "Design");
  console.log(\`All well-paid: \${allPaid}, Has designers: \${hasDesigner}\`);
}`,
  },
  {
    id: 'pattern-matching',
    label: 'Pattern Matching',
    rts: `type Shape =
  | { kind: "circle", radius: f64 }
  | { kind: "rect", width: f64, height: f64 }
  | { kind: "triangle", base: f64, height: f64 }

function area(shape: Shape): f64 {
  switch (shape) {
    case "circle":
      return 3.14159 * shape.radius * shape.radius;
    case "rect":
      return shape.width * shape.height;
    case "triangle":
      return 0.5 * shape.base * shape.height;
  }
}

type Direction = "north" | "south" | "east" | "west"

function opposite(dir: Direction): Direction {
  switch (dir) {
    case "north": return "south";
    case "south": return "north";
    case "east": return "west";
    case "west": return "east";
  }
}

function main() {
  const circle: Shape = { kind: "circle", radius: 5.0 };
  const rect: Shape = { kind: "rect", width: 4.0, height: 6.0 };
  const tri: Shape = { kind: "triangle", base: 3.0, height: 8.0 };

  console.log(\`circle: \${area(circle)}\`);
  console.log(\`rect: \${area(rect)}\`);
  console.log(\`triangle: \${area(tri)}\`);
  console.log(\`opposite of north: \${opposite("north")}\`);
}`,
  },
  {
    id: 'concurrency',
    label: 'Concurrency',
    rts: `/** Simulate a slow computation */
async function compute(label: string, value: i32): string {
  console.log(\`[\${label}] starting...\`);
  return \`\${label} = \${value}\`;
}

async function main() {
  // Sequential
  const a = await compute("task-1", 10);
  console.log(a);

  // Parallel with Promise.all
  const [b, c, d] = await Promise.all([
    compute("task-2", 20),
    compute("task-3", 30),
    compute("task-4", 40),
  ]);
  console.log(b);
  console.log(c);
  console.log(d);

  // Shared mutable state (Arc<Mutex<T>>)
  const counter: shared<i32> = shared(0);
  const val = counter.lock();
  console.log(\`Counter: \${val}\`);
}`,
  },
];
