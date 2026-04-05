# Fullstack API — RustScript Backend + TypeScript Frontend

A task tracker API demonstrating the fullstack type-sharing pattern: RustScript backend serves JSON via axum, TypeScript frontend consumes it with typed fetch wrappers, and types are shared via `rsc types`.

## Architecture

```
src/index.rts          RustScript backend (axum JSON API)
    |
    | rsc types -o frontend/src/types/
    v
frontend/src/types/    Generated .d.ts (shared type definitions)
    |
    v
frontend/src/api.ts    Typed fetch wrappers using shared types
```

## The Fullstack Type Pattern

1. Define types in RustScript with `derives Serialize`:

```typescript
type Task = {
  id: u32,
  title: string,
  status: string,
  priority: string,
  assignee: string | null,
} derives Serialize, Deserialize
```

2. `rsc types` generates TypeScript definitions:

```typescript
export interface Task {
  id: number;
  title: string;
  status: string;
  priority: string;
  assignee: string | null;
}
```

3. The frontend imports and uses the shared types:

```typescript
import type { Task } from './types';

const tasks = await fetchTasks();
tasks.forEach(t => console.log(t.title)); // fully typed
```

## Type Safety Across the Boundary

Change a field in the backend (e.g., rename `assignee` to `owner`), regenerate types with `rsc types`, and TypeScript catches every usage that needs updating. No runtime surprises.

## Building

```bash
# Build the RustScript backend and generate frontend types
rsc build --emit-types frontend/src/types/

# Or generate types separately
rsc types -o frontend/src/types/

# Start the API
rsc run

# Then use the typed API client in your frontend
```

## API Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /api/tasks` | All tasks |
| `GET /api/tasks/open` | Open tasks only |
| `GET /api/summary` | Summary statistics |
