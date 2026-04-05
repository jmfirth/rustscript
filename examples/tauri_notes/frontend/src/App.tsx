import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Note, Stats } from './types';

export default function App() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [stats, setStats] = useState<Stats | null>(null);
  const [query, setQuery] = useState('');

  useEffect(() => {
    invoke<Note[]>('get_notes').then(setNotes);
    invoke<Stats>('get_stats').then(setStats);
  }, []);

  const handleSearch = async () => {
    if (query.trim()) {
      const results = await invoke<Note[]>('search_notes', { query });
      setNotes(results);
    } else {
      const all = await invoke<Note[]>('get_notes');
      setNotes(all);
    }
  };

  return (
    <div style={{ padding: '2rem', fontFamily: 'system-ui', maxWidth: 600, margin: '0 auto' }}>
      <h1 style={{ marginBottom: '0.5rem' }}>Tauri Notes</h1>
      <p style={{ color: '#666', marginTop: 0, fontSize: '0.9rem' }}>
        Powered by RustScript + Tauri
      </p>

      {stats && (
        <p style={{ fontSize: '0.85rem', color: '#888' }}>
          {stats.total} notes, {stats.pinned} pinned
        </p>
      )}

      <div style={{ display: 'flex', gap: '0.5rem', marginBottom: '1.5rem' }}>
        <input
          type="text"
          placeholder="Search notes..."
          value={query}
          onChange={e => setQuery(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && handleSearch()}
          style={{
            flex: 1, padding: '0.5rem', borderRadius: 6,
            border: '1px solid #ddd', fontSize: '0.9rem',
          }}
        />
        <button
          onClick={handleSearch}
          style={{
            padding: '0.5rem 1rem', borderRadius: 6, border: 'none',
            background: '#e8750a', color: 'white', cursor: 'pointer',
            fontSize: '0.9rem', fontWeight: 600,
          }}
        >
          Search
        </button>
      </div>

      <ul style={{ listStyle: 'none', padding: 0 }}>
        {notes.map(note => (
          <li
            key={note.id}
            style={{
              padding: '1rem', marginBottom: '0.75rem', borderRadius: 8,
              border: '1px solid #eee', background: note.pinned ? '#fff8f0' : '#fff',
            }}
          >
            <strong>{note.pinned ? '\ud83d\udccc ' : ''}{note.title}</strong>
            <p style={{ margin: '0.25rem 0 0', color: '#555', fontSize: '0.9rem' }}>
              {note.content}
            </p>
          </li>
        ))}
      </ul>

      {notes.length === 0 && (
        <p style={{ color: '#999', textAlign: 'center' }}>No notes found.</p>
      )}
    </div>
  );
}
