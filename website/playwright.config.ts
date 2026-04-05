import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  timeout: 30000,
  webServer: {
    command: 'npx serve out -l 3123',
    port: 3123,
    reuseExistingServer: !process.env.CI,
  },
  use: {
    baseURL: 'http://localhost:3123',
  },
});
