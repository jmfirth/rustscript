import { useState, useEffect } from 'react';
import type { Note, NoteStats } from './types';

// In a real Tauri app, this would use @tauri-apps/api
// import { invoke } from '@tauri-apps/api/core';

// Simulated Tauri invoke for the example
async function invoke(cmd: string, args?: Record<string, unknown>): Promise<string> {
  console.log(`[tauri] invoke: ${cmd}`, args);
  return '[]'; // placeholder
}

export default function App() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [stats, setStats] = useState<NoteStats | null>(null);
  const [search, setSearch] = useState('');

  useEffect(() => {
    // Load notes on mount
    invoke('get_all_notes').then(json => {
      setNotes(JSON.parse(json) as Note[]);
    });
    invoke('get_stats').then(json => {
      setStats(JSON.parse(json) as NoteStats);
    });
  }, []);

  const handleSearch = async () => {
    const json = await invoke('search_notes', { query: search });
    setNotes(JSON.parse(json) as Note[]);
  };

  return (
    <div style={{ padding: '2rem', fontFamily: 'system-ui' }}>
      <h1>Notes</h1>
      {stats && (
        <p>{stats.total} notes, {stats.pinned} pinned</p>
      )}
      <div>
        <input
          value={search}
          onChange={e => setSearch(e.target.value)}
          placeholder="Search notes..."
        />
        <button onClick={handleSearch}>Search</button>
      </div>
      <ul>
        {notes.map(note => (
          <li key={note.id}>
            <strong>{note.pinned ? '📌 ' : ''}{note.title}</strong>
            <p>{note.content}</p>
            <small>{note.created_at}</small>
          </li>
        ))}
      </ul>
    </div>
  );
}
