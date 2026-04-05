/**
 * RustScript compiler — WASM interface.
 *
 * Wraps the rsc-web WASM module with typed functions for use in
 * the playground and crate docs viewer.
 *
 * Usage:
 *   import { initCompiler, compile, hover, translateRustdoc } from '@/lib/rsc-compiler';
 *   await initCompiler();
 *   const result = compile('function main() { console.log("hello"); }');
 */

import init, {
  compile as wasmCompile,
  get_diagnostics as wasmGetDiagnostics,
  hover as wasmHover,
  translate_rustdoc as wasmTranslateRustdoc,
} from '@/wasm/rsc_web';

let initialized = false;

/** Initialize the WASM module. Must be called once before using other functions. */
export async function initCompiler(): Promise<void> {
  if (initialized) return;
  await init();
  initialized = true;
}

/** Whether the compiler has been initialized. */
export function isInitialized(): boolean {
  return initialized;
}

// --- Types ---

export interface CompileResult {
  rust_source: string;
  diagnostics: Diagnostic[];
  has_errors: boolean;
}

export interface Diagnostic {
  message: string;
  severity: 'error' | 'warning' | 'info';
  line: number | null;
  column: number | null;
}

export interface TranslatedItem {
  name: string;
  kind: 'function' | 'struct' | 'trait' | 'enum';
  signature: string;
  docs: string | null;
  module: string | null;
  is_trait_impl: boolean;
  is_public_api: boolean;
}

// --- API ---

/** Compile RustScript source to Rust. */
export function compile(source: string): CompileResult {
  return wasmCompile(source) as CompileResult;
}

/** Get diagnostics only (faster than full compile — skips emit). */
export function getDiagnostics(source: string): Diagnostic[] {
  return wasmGetDiagnostics(source) as Diagnostic[];
}

/** Get hover info for symbol at position. */
export function getHover(source: string, line: number, column: number): string {
  return wasmHover(source, line, column);
}

/** Translate rustdoc JSON to RustScript-syntax items. */
export function translateRustdoc(json: string): TranslatedItem[] {
  return wasmTranslateRustdoc(json) as TranslatedItem[];
}
