// End-to-end: the built UI against the real server, which Playwright starts.
//
//   cd backend && cargo build --release
//   cd frontend && npm run build && npm run test:e2e
//
// SCHEMGEN_BINARY points at another server build; PLAYWRIGHT_CHROMIUM at a
// Chromium to use instead of Playwright's own.

import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { defineConfig, devices } from '@playwright/test'

const port = Number(process.env.SCHEMGEN_E2E_PORT || 3411)
const binary = process.env.SCHEMGEN_BINARY || '../backend/target/release/schemgen2'
const workDir = join(tmpdir(), `schemgen-e2e-${port}`)

export default defineConfig({
  testDir: 'e2e',
  timeout: 120_000,
  retries: 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: 'retain-on-failure',
    launchOptions: {
      executablePath: process.env.PLAYWRIGHT_CHROMIUM || undefined,
      // WebGL without a GPU.
      args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
    },
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1400, height: 900 } },
    },
  ],
  webServer: {
    command: `${binary} serve --port ${port} --ui-dir dist --work-dir ${workDir} --job-ttl 0`,
    url: `http://127.0.0.1:${port}/api/health`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    stdout: 'ignore',
    stderr: 'pipe',
  },
})
