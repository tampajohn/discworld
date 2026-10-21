import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests',
  testMatch: '**/*.spec.js',
  timeout: 120000,
  workers: 1,
  projects: [
    { name: 'chrome', use: { browserName: 'chromium', channel: 'chrome' } },
    { name: 'webkit', use: { browserName: 'webkit' } },
  ],
  use: { baseURL: 'http://127.0.0.1:8766', headless: true,
    viewport: { width: 1024, height: 768 }, hasTouch: true, acceptDownloads: true },
  webServer: { command: 'python3 -m http.server 8766 --bind 127.0.0.1 --directory .', url: 'http://127.0.0.1:8766', reuseExistingServer: false },
});
