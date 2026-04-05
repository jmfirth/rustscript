'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTheme } from 'next-themes';
import Editor, { type Monaco, type OnMount } from '@monaco-editor/react';
import type { editor as monacoEditor, IPosition } from 'monaco-editor';
import {
  rustscriptLanguageId,
  rustscriptLanguageConfig,
  rustscriptMonarchLanguage,
} from '@/lib/rustscript-monarch';
import { examples, type PlaygroundExample } from '@/lib/playground-examples';
import { useCompiler } from '@/lib/use-compiler';
import type { Diagnostic } from '@/lib/rsc-compiler';

const LIGHT_THEME = 'rustscript-light';
const DARK_THEME = 'rustscript-dark';

type CompilationStatus =
  | { kind: 'initializing' }
  | { kind: 'ready' }
  | { kind: 'compiling' }
  | { kind: 'success' }
  | { kind: 'errors'; count: number };

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

function severityToMonaco(severity: Diagnostic['severity'], monaco: Monaco): number {
  switch (severity) {
    case 'error':
      return monaco.MarkerSeverity.Error;
    case 'warning':
      return monaco.MarkerSeverity.Warning;
    case 'info':
      return monaco.MarkerSeverity.Info;
    default:
      return monaco.MarkerSeverity.Info;
  }
}

function statusText(status: CompilationStatus): string {
  switch (status.kind) {
    case 'initializing':
      return 'Initializing compiler...';
    case 'ready':
      return 'Ready';
    case 'compiling':
      return 'Compiling...';
    case 'success':
      return 'Compiled successfully';
    case 'errors':
      return `${status.count} error${status.count === 1 ? '' : 's'}`;
  }
}

function statusColor(status: CompilationStatus): string {
  switch (status.kind) {
    case 'initializing':
    case 'compiling':
      return 'var(--color-text-secondary)';
    case 'ready':
      return 'var(--color-text-secondary)';
    case 'success':
      return '#16A34A';
    case 'errors':
      return '#DC2626';
  }
}

const DEBOUNCE_MS = 300;

export function PlaygroundEditor() {
  const { resolvedTheme } = useTheme();
  const [mounted, setMounted] = useState(false);
  const [selectedExample, setSelectedExample] = useState<PlaygroundExample>(examples[0]);
  const [rtsCode, setRtsCode] = useState(examples[0].rts);
  const [rustOutput, setRustOutput] = useState('// Initializing compiler...');
  const [status, setStatus] = useState<CompilationStatus>({ kind: 'initializing' });
  const [banner, setBanner] = useState<string | null>(null);

  const monacoRef = useRef<Monaco | null>(null);
  const rtsEditorRef = useRef<monacoEditor.IStandaloneCodeEditor | null>(null);
  const bannerTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const compileTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverDisposableRef = useRef<{ dispose: () => void } | null>(null);

  const compiler = useCompiler();

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

  // Compile function that updates output and diagnostics
  const doCompile = useCallback(async (source: string) => {
    if (!compiler.ready) return;

    setStatus({ kind: 'compiling' });

    try {
      const result = await compiler.compile(source);

      setRustOutput(result.rust_source || '// No output');

      // Set markers on the RustScript editor
      if (monacoRef.current && rtsEditorRef.current) {
        const model = rtsEditorRef.current.getModel();
        if (model) {
          const markers = result.diagnostics.map((d: Diagnostic) => ({
            severity: severityToMonaco(d.severity, monacoRef.current!),
            message: d.message,
            startLineNumber: d.line ?? 1,
            startColumn: d.column ?? 1,
            endLineNumber: d.line ?? 1,
            endColumn: (d.column ?? 1) + 1,
          }));
          monacoRef.current.editor.setModelMarkers(model, 'rustscript', markers);
        }
      }

      const errorCount = result.diagnostics.filter(
        (d: Diagnostic) => d.severity === 'error'
      ).length;

      if (errorCount > 0) {
        setStatus({ kind: 'errors', count: errorCount });
      } else {
        setStatus({ kind: 'success' });
      }
    } catch (err) {
      setRustOutput(`// Compilation error: ${err}`);
      setStatus({ kind: 'errors', count: 1 });
    }
  }, [compiler]);

  // Keep a ref to compiler.ready so the hover provider can check it
  const compilerReadyRef = useRef(false);
  useEffect(() => {
    compilerReadyRef.current = compiler.ready;
  }, [compiler.ready]);

  // When compiler becomes ready, compile the initial example
  useEffect(() => {
    if (compiler.ready) {
      setStatus({ kind: 'ready' });
      doCompile(rtsCode);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [compiler.ready]);

  // Debounced compilation on code change
  const handleCodeChange = useCallback(
    (value: string | undefined) => {
      const source = value ?? '';
      setRtsCode(source);

      if (compileTimeoutRef.current) {
        clearTimeout(compileTimeoutRef.current);
      }

      compileTimeoutRef.current = setTimeout(() => {
        doCompile(source);
      }, DEBOUNCE_MS);
    },
    [doCompile]
  );

  const handleBeforeMount = useCallback((monaco: Monaco) => {
    monacoRef.current = monaco;
    registerRustScriptLanguage(monaco);
    defineThemes(monaco);
  }, []);

  const handleRtsBeforeMount = useCallback((monaco: Monaco) => {
    handleBeforeMount(monaco);

    // Register hover provider only for the RustScript editor
    if (!hoverDisposableRef.current) {
      hoverDisposableRef.current = monaco.languages.registerHoverProvider(
        rustscriptLanguageId,
        {
          provideHover: async (model: monacoEditor.ITextModel, position: IPosition) => {
            if (!compilerReadyRef.current) return null;
            const source = model.getValue();
            try {
              const info = await compiler.getHover(
                source,
                position.lineNumber,
                position.column
              );
              if (info && info.trim().length > 0) {
                return {
                  range: new monaco.Range(
                    position.lineNumber,
                    position.column,
                    position.lineNumber,
                    position.column
                  ),
                  contents: [{ value: info, isTrusted: true }],
                };
              }
            } catch {
              // Hover failed silently
            }
            return null;
          },
        }
      );
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [handleBeforeMount]);

  const handleEditorMount: OnMount = useCallback((editor) => {
    rtsEditorRef.current = editor;
  }, []);

  const handleExampleChange = useCallback(
    (e: React.ChangeEvent<HTMLSelectElement>) => {
      const example = examples.find((ex) => ex.id === e.target.value);
      if (example) {
        setSelectedExample(example);
        setRtsCode(example.rts);
        doCompile(example.rts);
      }
    },
    [doCompile]
  );

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
      await navigator.clipboard.writeText(rustOutput);
      setBanner('Rust output copied to clipboard');
      if (bannerTimeoutRef.current) clearTimeout(bannerTimeoutRef.current);
      bannerTimeoutRef.current = setTimeout(() => setBanner(null), 2000);
    } catch {
      // Clipboard API not available
    }
  }, [rustOutput]);

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
            <span className="inline-block w-2 h-2 rounded-full bg-[var(--color-accent)] mr-2" />
            RustScript (.rts)
          </div>
          <div className="flex-1 min-h-0">
            <Editor
              language={rustscriptLanguageId}
              value={rtsCode}
              onChange={handleCodeChange}
              theme={monacoTheme}
              beforeMount={handleRtsBeforeMount}
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
            <span className="inline-block w-2 h-2 rounded-full bg-[var(--color-accent-secondary)] mr-2" />
            Rust (generated .rs)
          </div>
          <div className="flex-1 min-h-0">
            <Editor
              language="rust"
              value={rustOutput}
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
      <div className="flex items-center justify-between px-4 py-1.5 text-xs bg-[var(--color-bg-secondary)] border-t border-[var(--color-border)] shrink-0">
        <span className="text-[var(--color-text-secondary)]">
          Example: {selectedExample.label}
        </span>
        <span style={{ color: statusColor(status) }}>
          {statusText(status)}
        </span>
      </div>
    </div>
  );
}
