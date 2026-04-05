import type { Task, TaskSummary } from './types';

const BASE = 'http://localhost:3001';

export async function fetchTasks(): Promise<Task[]> {
  const res = await fetch(`${BASE}/api/tasks`);
  return res.json() as Promise<Task[]>;
}

export async function fetchOpenTasks(): Promise<Task[]> {
  const res = await fetch(`${BASE}/api/tasks/open`);
  return res.json() as Promise<Task[]>;
}

export async function fetchSummary(): Promise<TaskSummary> {
  const res = await fetch(`${BASE}/api/summary`);
  return res.json() as Promise<TaskSummary>;
}

// Usage in a React/Vue/Svelte component:
//
//   const tasks = await fetchTasks();
//   tasks.forEach(t => console.log(t.title)); // <- fully typed!
//
// If the backend type changes, `rsc types` regenerates the .d.ts,
// and TypeScript catches the mismatch at compile time.
