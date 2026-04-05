/**
 * RustScript compiler Web Worker.
 *
 * Runs the WASM compiler off the main thread. Both the playground
 * and crate docs viewer communicate with this worker via postMessage.
 */

import init, {
  compile,
  get_diagnostics,
  hover,
  translate_rustdoc,
} from '@/wasm/rsc_web';

let ready = false;

async function initialize() {
  try {
    console.log('[rsc-worker] Initializing WASM...');
    await init();
    ready = true;
    console.log('[rsc-worker] WASM initialized successfully');
    self.postMessage({ type: 'ready' });
  } catch (err) {
    console.error('[rsc-worker] WASM initialization failed:', err);
    self.postMessage({ type: 'error', id: -1, error: `WASM init failed: ${err}` });
  }
}

self.onmessage = async (e: MessageEvent<WorkerRequest>) => {
  if (!ready) {
    await initialize();
  }

  const { id, type, payload } = e.data;

  try {
    let result: unknown;

    switch (type) {
      case 'compile':
        result = compile(payload.source as string);
        break;
      case 'diagnostics':
        result = get_diagnostics(payload.source as string);
        break;
      case 'hover':
        result = hover(payload.source as string, payload.line as number, payload.column as number);
        break;
      case 'translate':
        result = translate_rustdoc(payload.json as string);
        break;
      default:
        self.postMessage({ id, type: 'error', error: `Unknown request type: ${type}` });
        return;
    }

    self.postMessage({ id, type: 'result', result });
  } catch (err) {
    self.postMessage({ id, type: 'error', error: String(err) });
  }
};

// Start initialization immediately
initialize();

// --- Shared types (also used by the client) ---

export interface WorkerRequest {
  id: number;
  type: 'compile' | 'diagnostics' | 'hover' | 'translate';
  payload: Record<string, unknown>;
}

export interface WorkerResponse {
  id: number;
  type: 'result' | 'error' | 'ready';
  result?: unknown;
  error?: string;
}
