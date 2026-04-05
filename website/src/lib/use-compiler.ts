/**
 * React hook for the RustScript compiler worker.
 *
 * Usage:
 *   const compiler = useCompiler();
 *   const result = await compiler.compile(source);
 *   const items = await compiler.translateRustdoc(json);
 */

'use client';

import { useEffect, useRef, useCallback, useState } from 'react';
import type { CompileResult, Diagnostic, TranslatedItem } from './rsc-compiler';
import type { WorkerResponse } from './rsc-worker';

let nextId = 0;

export function useCompiler() {
  const workerRef = useRef<Worker | null>(null);
  const pendingRef = useRef<Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>>(new Map());
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const worker = new Worker(new URL('./rsc-worker.ts', import.meta.url), {
      type: 'module',
    });

    worker.onerror = (e) => {
      console.error('[useCompiler] Worker error:', e);
    };

    worker.onmessage = (e: MessageEvent<WorkerResponse>) => {
      const { id, type, result, error } = e.data;

      if (type === 'ready') {
        console.log('[useCompiler] Compiler ready');
        setReady(true);
        return;
      }

      const pending = pendingRef.current.get(id);
      if (!pending) return;
      pendingRef.current.delete(id);

      if (type === 'error') {
        pending.reject(new Error(error ?? 'Unknown worker error'));
      } else {
        pending.resolve(result);
      }
    };

    workerRef.current = worker;

    return () => {
      worker.terminate();
      workerRef.current = null;
    };
  }, []);

  const send = useCallback((type: string, payload: Record<string, unknown>): Promise<unknown> => {
    return new Promise((resolve, reject) => {
      const id = nextId++;
      pendingRef.current.set(id, { resolve, reject });
      workerRef.current?.postMessage({ id, type, payload });
    });
  }, []);

  const compile = useCallback(
    (source: string) => send('compile', { source }) as Promise<CompileResult>,
    [send]
  );

  const getDiagnostics = useCallback(
    (source: string) => send('diagnostics', { source }) as Promise<Diagnostic[]>,
    [send]
  );

  const getHover = useCallback(
    (source: string, line: number, column: number) =>
      send('hover', { source, line, column }) as Promise<string>,
    [send]
  );

  const translateRustdoc = useCallback(
    (json: string) => send('translate', { json }) as Promise<TranslatedItem[]>,
    [send]
  );

  return { ready, compile, getDiagnostics, getHover, translateRustdoc };
}
