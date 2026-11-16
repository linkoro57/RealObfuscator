import { defineConfig } from '@playwright/test';
import { existsSync } from 'node:fs';
const edge = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
export default defineConfig({
  testDir: './web/tests',
  timeout: 60000,
  workers: 1,
  use: {
    baseURL: 'http://127.0.0.1:4173',
    browserName: 'chromium',
    launchOptions: process.platform === 'win32' && existsSync(edge) ? { executablePath: edge } : {},
    trace: 'retain-on-failure',
  },
  webServer: { command: 'npm run preview:web', url: 'http://127.0.0.1:4173', reuseExistingServer: !process.env.CI },
});
