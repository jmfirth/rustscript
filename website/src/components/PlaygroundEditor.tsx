'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTheme } from 'next-themes';
import Editor, { type Monaco, type OnMount } from '@monaco-editor/react';
import {
  rustscriptLanguageId,
  rustscriptLanguageConfig,
  rustscriptMonarchLanguage,
} from '@/lib/rustscript-monarch';
import { examples, type PlaygroundExample } from '@/lib/playground-examples';

const LIGHT_THEME = 'rustscript-light';
const DARK_THEME = 'rustscript-dark';

function defineThemes(monaco: Monaco) {
  monaco.editor.defineTheme(LIGHT_THEME, {
    base: 'vs',
    inherit: true,
    rules: [
      { token: 'keyword', foreground: 'CE422B', fontStyle: 'bold' },
      { token: 'type', foreground: '4B7BEC' },
      { token: 'variable.predefined', foreground: '8B5CF6' },
      { token: 'string', foreground: '16A34A' },
      { token: 'string.template', foreground: '16A34A' },
      { token: 'string.escape', foreground: 'D97706' },
      { token: 'number', foreground: 'B45309' },
      { token: 'number.float', foreground: 'B45309' },
      { token: 'number.hex', foreground: 'B45309' },
      { token: 'comment', foreground: '9CA3AF', fontStyle: 'italic' },
      { token: 'operator', foreground: 'CE422B' },
      { token: 'delimiter', foreground: '555555' },
      { token: 'delimiter.bracket', foreground: 'CE422B' },
      { token: 'regexp', foreground: 'D97706' },
      { token: 'regexp.escape.control', foreground: 'B45309' },
    ],
    colors: {
      'editor.background': '#FAFAFA',
      'editor.foreground': '#1A1A1A',
      'editor.lineHighlightBackground': '#F0F0F0',
      'editorLineNumber.foreground': '#AAAAAA',
      'editorLineNumber.activeForeground': '#555555',
      'editor.selectionBackground': '#4B7BEC33',
      'editor.inactiveSelectionBackground': '#4B7BEC1A',
      'editorCursor.foreground': '#CE422B',
      'editorIndentGuide.background': '#E0E0E0',
      'editorIndentGuide.activeBackground': '#CCCCCC',
    },
  });

  monaco.editor.defineTheme(DARK_THEME, {
    base: 'vs-dark',
    inherit: true,
    rules: [
      { token: 'keyword', foreground: 'E8654A', fontStyle: 'bold' },
      { token: 'type', foreground: '6B9BFF' },
      { token: 'variable.predefined', foreground: 'A78BFA' },
      { token: 'string', foreground: '4ADE80' },
      { token: 'string.template', foreground: '4ADE80' },
      { token: 'string.escape', foreground: 'FBBF24' },
      { token: 'number', foreground: 'F59E0B' },
      { token: 'number.float', foreground: 'F59E0B' },
      { token: 'number.hex', foreground: 'F59E0B' },
      { token: 'comment', foreground: '6B7280', fontStyle: 'italic' },
      { token: 'operator', foreground: 'E8654A' },
      { token: 'delimiter', foreground: 'A0A0A0' },
      { token: 'delimiter.bracket', foreground: 'E8654A' },
      { token: 'regexp', foreground: 'FBBF24' },
      { token: 'regexp.escape.control', foreground: 'F59E0B' },
    ],
    colors: {
      'editor.background': '#111111',
      'editor.foreground': '#E5E5E5',
      'editor.lineHighlightBackground': '#1A1A1A',
      'editorLineNumber.foreground': '#555555',
      'editorLineNumber.activeForeground': '#A0A0A0',
      'editor.selectionBackground': '#4B7BEC44',
      'editor.inactiveSelectionBackground': '#4B7BEC22',
      'editorCursor.foreground': '#E8654A',
      'editorIndentGuide.background': '#2A2A2A',
      'editorIndentGuide.activeBackground': '#3A3A3A',
    },
  });
}

function registerRustScriptLanguage(monaco: Monaco) {
  if (!monaco.languages.getLanguages().some((lang: { id: string }) => lang.id === rustscriptLanguageId)) {
    monaco.languages.register({ id: rustscriptLanguageId, extensions: ['.rts'] });
    monaco.languages.setLanguageConfiguration(rustscriptLanguageId, rustscriptLanguageConfig);
    monaco.languages.setMonarchTokensProvider(rustscriptLanguageId, rustscriptMonarchLanguage);
  }
}

export function PlaygroundEditor() {
  const { resolvedTheme } = useTheme();
  const [mounted, setMounted] = useState(false);
  const [selectedExample, setSelectedExample] = useState<PlaygroundExample>(examples[0]);
  const [rtsCode, setRtsCode] = useState(examples[0].rts);
  const [banner, setBanner] = useState<string | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const bannerTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    setMounted(true);
  }, []);

  // Switch Monaco theme when site theme changes
  useEffect(() => {
    if (monacoRef.current && mounted) {
      monacoRef.current.editor.setTheme(
        resolvedTheme === 'dark' ? DARK_THEME : LIGHT_THEME
      );
    }
  }, [resolvedTheme, mounted]);

  const handleBeforeMount = useCallback((monaco: Monaco) => {
    monacoRef.current = monaco;
    registerRustScriptLanguage(monaco);
    defineThemes(monaco);
  }, []);

  const handleEditorMount: OnMount = useCallback(() => {
    // Editor is ready
  }, []);

  const handleExampleChange = useCallback((e: React.ChangeEvent<HTMLSelectElement>) => {
    const example = examples.find((ex) => ex.id === e.target.value);
    if (example) {
      setSelectedExample(example);
      setRtsCode(example.rts);
    }
  }, []);

  const handleCompile = useCallback(() => {
    setBanner('WASM compilation coming soon -- this is a preview of the editor experience');
    if (bannerTimeoutRef.current) {
      clearTimeout(bannerTimeoutRef.current);
    }
    bannerTimeoutRef.current = setTimeout(() => setBanner(null), 4000);
  }, []);

  const handleCopyRts = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(rtsCode);
      setBanner('RustScript code copied to clipboard');
      if (bannerTimeoutRef.current) clearTimeout(bannerTimeoutRef.current);
      bannerTimeoutRef.current = setTimeout(() => setBanner(null), 2000);
    } catch {
      // Clipboard API not available
    }
  }, [rtsCode]);

  const handleCopyRs = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(selectedExample.rs);
      setBanner('Rust output copied to clipboard');
      if (bannerTimeoutRef.current) clearTimeout(bannerTimeoutRef.current);
      bannerTimeoutRef.current = setTimeout(() => setBanner(null), 2000);
    } catch {
      // Clipboard API not available
    }
  }, [selectedExample.rs]);

  const monacoTheme = resolvedTheme === 'dark' ? DARK_THEME : LIGHT_THEME;

  const editorOptions = {
    fontSize: 14,
    fontFamily: "'JetBrains Mono', 'Fira Code', ui-monospace, monospace",
    lineNumbers: 'on' as const,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    padding: { top: 12, bottom: 12 },
    renderLineHighlight: 'line' as const,
    automaticLayout: true,
    tabSize: 2,
    wordWrap: 'on' as const,
    overviewRulerLanes: 0,
    hideCursorInOverviewRuler: true,
    overviewRulerBorder: false,
    scrollbar: {
      verticalScrollbarSize: 8,
      horizontalScrollbarSize: 8,
    },
  };

  if (!mounted) {
    return (
      <div className="flex-1 flex items-center justify-center text-[var(--color-text-secondary)]">
        Loading editor...
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-3 px-4 py-2 border-b border-[var(--color-border)] bg-[var(--color-bg)] shrink-0">
        <select
          value={selectedExample.id}
          onChange={handleExampleChange}
          className="h-8 px-3 text-sm rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text)] focus:outline-none focus:ring-2 focus:ring-[var(--color-accent)]/40"
        >
          {examples.map((ex) => (
            <option key={ex.id} value={ex.id}>
              {ex.label}
            </option>
          ))}
        </select>

        <button
          onClick={handleCompile}
          className="h-8 px-4 text-sm font-medium rounded-md bg-[var(--color-accent)] text-white hover:opacity-90 transition-opacity"
        >
          Compile
        </button>

        <div className="flex-1" />

        <button
          onClick={handleCopyRts}
          className="h-8 px-3 text-sm rounded-md border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:text-[var(--color-text)] hover:bg-[var(--color-bg-secondary)] transition-colors"
          title="Copy RustScript code"
        >
          Copy .rts
        </button>
        <button
          onClick={handleCopyRs}
          className="h-8 px-3 text-sm rounded-md border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:text-[var(--color-text)] hover:bg-[var(--color-bg-secondary)] transition-colors"
          title="Copy generated Rust code"
        >
          Copy .rs
        </button>
      </div>

      {/* Banner */}
      {banner && (
        <div className="px-4 py-2 text-sm text-center bg-[var(--color-accent)]/10 text-[var(--color-accent)] border-b border-[var(--color-accent)]/20 shrink-0">
          {banner}
        </div>
      )}

      {/* Editor Panes */}
      <div className="flex-1 flex flex-col md:flex-row min-h-0">
        {/* Left: RustScript */}
        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          <div className="flex items-center px-4 py-1.5 text-xs font-medium text-[var(--color-text-secondary)] bg-[var(--color-bg-secondary)] border-b border-[var(--color-border)] shrink-0">
            <span className="inline-block w-2 h-2 rounded-full bg-[var(--color-accent-secondary)] mr-2" />
            RustScript (.rts)
          </div>
          <div className="flex-1 min-h-0">
            <Editor
              language={rustscriptLanguageId}
              value={rtsCode}
              onChange={(value) => setRtsCode(value ?? '')}
              theme={monacoTheme}
              beforeMount={handleBeforeMount}
              onMount={handleEditorMount}
              options={editorOptions}
              loading={
                <div className="flex items-center justify-center h-full text-[var(--color-text-secondary)] text-sm">
                  Loading editor...
                </div>
              }
            />
          </div>
        </div>

        {/* Divider */}
        <div className="hidden md:block w-px bg-[var(--color-border)] shrink-0" />
        <div className="md:hidden h-px bg-[var(--color-border)] shrink-0" />

        {/* Right: Rust output */}
        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          <div className="flex items-center px-4 py-1.5 text-xs font-medium text-[var(--color-text-secondary)] bg-[var(--color-bg-secondary)] border-b border-[var(--color-border)] shrink-0">
            <span className="inline-block w-2 h-2 rounded-full bg-[var(--color-accent)] mr-2" />
            Rust (generated .rs)
          </div>
          <div className="flex-1 min-h-0">
            <Editor
              language="rust"
              value={selectedExample.rs}
              theme={monacoTheme}
              beforeMount={handleBeforeMount}
              options={{
                ...editorOptions,
                readOnly: true,
                domReadOnly: true,
              }}
              loading={
                <div className="flex items-center justify-center h-full text-[var(--color-text-secondary)] text-sm">
                  Loading editor...
                </div>
              }
            />
          </div>
        </div>
      </div>

      {/* Status bar */}
      <div className="flex items-center justify-between px-4 py-1.5 text-xs text-[var(--color-text-secondary)] bg-[var(--color-bg-secondary)] border-t border-[var(--color-border)] shrink-0">
        <span>
          Example: {selectedExample.label}
        </span>
        <span>
          RustScript Playground (Preview)
        </span>
      </div>
    </div>
  );
}
