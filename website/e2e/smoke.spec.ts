import { test, expect } from '@playwright/test';

test.describe('Site smoke tests', () => {
  test('landing page loads', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('h1')).toContainText('Write TypeScript');
  });

  test('docs page loads', async ({ page }) => {
    await page.goto('/docs');
    await expect(page.locator('h1')).toBeVisible();
  });

  test('playground page loads', async ({ page }) => {
    await page.goto('/playground');
    await expect(page.locator('.monaco-editor').first()).toBeVisible({ timeout: 10000 });
  });

  test('crates page loads', async ({ page }) => {
    await page.goto('/crates');
    await expect(page.locator('text=axum')).toBeVisible();
  });
});

test.describe('Playground WASM integration', () => {
  test('playground initializes Monaco editor', async ({ page }) => {
    await page.goto('/playground');
    // Monaco renders into a div with class 'monaco-editor'
    await expect(page.locator('.monaco-editor').first()).toBeVisible({ timeout: 10000 });
  });

  test('WASM compiler loads and compiles', async ({ page }) => {
    await page.goto('/playground');

    // Wait for the page to be interactive
    await page.waitForLoadState('networkidle');

    // Evaluate the WASM compiler directly in the browser
    const result = await page.evaluate(async () => {
      const wasm = await import('/wasm/rsc_web.js');
      await wasm.default();
      const output = wasm.compile('function main() { console.log("hello"); }');
      return output;
    });

    expect(result).toBeTruthy();
    expect(result.has_errors).toBe(false);
    expect(result.rust_source).toContain('println!');
  });

  test('WASM hover returns builtin info', async ({ page }) => {
    await page.goto('/playground');
    await page.waitForLoadState('networkidle');

    const hoverText = await page.evaluate(async () => {
      const wasm = await import('/wasm/rsc_web.js');
      await wasm.default();
      return wasm.hover('console.log("hello");', 0, 9);
    });

    expect(hoverText).toContain('console.log');
  });

  test('WASM diagnostics reports errors', async ({ page }) => {
    await page.goto('/playground');
    await page.waitForLoadState('networkidle');

    const diagnostics = await page.evaluate(async () => {
      const wasm = await import('/wasm/rsc_web.js');
      await wasm.default();
      return wasm.get_diagnostics('function main( { }');
    });

    expect(Array.isArray(diagnostics)).toBe(true);
    expect(diagnostics.length).toBeGreaterThan(0);
  });
});
