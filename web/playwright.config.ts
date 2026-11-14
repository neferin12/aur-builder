import { defineConfig } from '@playwright/test'
export default defineConfig({
  testDir: './tests/e2e', fullyParallel: false, workers: 1,
  use: { baseURL: 'https://localhost:3443', ignoreHTTPSErrors: true },
  webServer: { command: 'node tests/e2e-server.mjs', url: 'https://localhost:3443/auth/signed-out', ignoreHTTPSErrors: true, reuseExistingServer: false, timeout: 120000 },
})
