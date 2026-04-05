import { useState, useEffect } from 'react';
import type { Note, Stats } from './types';

// In a real Tauri app: import { invoke } from '@tauri-apps/api/core';
// Simulated for the example:
async function invoke<T>(cmd: string): Promise<T> {
  const responses: Record<string, unknown> = {
    get_notes: [
      { id: 1, title: "Welcome", content: "Welcome to RustScript + Tauri!", pinned: true },
      { id: 2, title: "Getting Started", content: "Edit src/main.rts to add backend commands.", pinned: false },
    ],
    get_stats: { total: 2, pinned: 1 },
  };
  return responses[cmd] as T;
}

export default function App() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [stats, setStats] = useState<Stats | null>(null);

  useEffect(() => {
    invoke<Note[]>('get_notes').then(setNotes);
    invoke<Stats>('get_stats').then(setStats);
  }, []);

  return (
    <div style={{ padding: '2rem', fontFamily: 'system-ui' }}>
      <h1>Notes</h1>
      {stats && <p>{stats.total} notes, {stats.pinned} pinned</p>}
      <ul>
        {notes.map(note => (
          <li key={note.id}>
            <strong>{note.pinned ? '\ud83d\udccc ' : ''}{note.title}</strong>
            <p>{note.content}</p>
          </li>
        ))}
      </ul>
    </div>
  );
}
