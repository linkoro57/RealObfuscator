import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import AxeBuilder from '@axe-core/playwright';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(page.getByRole('button', { name: 'Obfuscate', exact: true })).toBeEnabled({ timeout: 45000 });
});

test('real WASM engine, exact seed, stats and download across every preset', async ({ page }) => {
  const external = [], errors = [];
  page.on('request', request => { if (!request.url().startsWith('http://127.0.0.1:4173')) external.push(request.url()); });
  page.on('pageerror', error => errors.push(error.message));
  await page.locator('#source').fill("local function f(x)return x+17,'hello' end;print(f(2))");
  await page.locator('#seed').fill('18446744073709551615');
  let vmOutput;
  for (const preset of ['rename', 'lightweight', 'native', 'vm', 'hardened']) {
    await page.locator('#preset').selectOption(preset);
    await page.locator('#obfuscate').click();
    await expect(page.locator('#run-state')).toHaveText('Done');
    await expect(page.locator('#output')).not.toHaveValue('');
    await expect(page.locator('#error')).toBeHidden();
    const result = await page.locator('#output').inputValue();
    expect(result).toContain('--!nocheck');
    const pending = page.waitForEvent('download');
    await page.locator('#download').click();
    const download = await pending;
    expect(download.suggestedFilename()).toBe('script.obf.luau');
    expect(readFileSync(await download.path(), 'utf8')).toBe(result);
    if (preset === 'hardened') {
      vmOutput = result;
      await expect(page.locator('#stats')).toContainText('vm_chunks');
      await page.locator('#obfuscate').click();
      await expect(page.locator('#run-state')).toHaveText('Done');
      await expect(page.locator('#output')).toHaveValue(vmOutput);
    }
  }
  const reportDownload = page.waitForEvent('download');
  await page.locator('#download-report').click();
  const report = JSON.parse(readFileSync(await (await reportDownload).path(), 'utf8'));
  expect(report.report.vm_chunks).toBe(1);
  expect(report.report.vm_functions).toBe(1);
  expect(report.report.vm_bound_strings).toBeGreaterThan(0);
  expect(report.warnings).toEqual([]);
  expect(external).toEqual([]);
  expect(errors).toEqual([]);
  // On developer machines, execute the real browser output in official Luau.
  const luau = resolve('tests/runtime/0.741/luau.exe');
  if (process.platform === 'win32') {
    const { existsSync, writeFileSync, mkdirSync } = await import('node:fs');
    if (existsSync(luau)) {
      mkdirSync('target/web-tests', { recursive: true });
      writeFileSync('target/web-tests/output.luau', vmOutput);
      const run = spawnSync(luau, ['target/web-tests/output.luau'], { encoding: 'utf8', timeout: 10000 });
      expect(run.status).toBe(0); expect(run.stdout.trim()).toBe('19\thello');
    }
  }
});

test('syntax errors, invalid combinations and stale result handling', async ({ page }) => {
  await page.locator('#obfuscate').click(); await expect(page.locator('#run-state')).toHaveText('Done');
  await page.locator('#source').fill('local = broken');
  await expect(page.locator('#download')).toBeDisabled();
  await page.locator('#obfuscate').click();
  await expect(page.locator('#error')).toBeVisible();
  await expect(page.locator('#output')).toHaveValue('');
  await page.locator('#source').fill('return 1');
  await page.locator('#open-options').click();
  await page.locator('#option-name_min_length').fill('30');
  await page.locator('#option-name_max_length').fill('2');
  await page.locator('#close-options').click();
  await page.locator('#obfuscate').click();
  await expect(page.locator('#error')).toContainText('name_max_length');
  await page.locator('#open-options').click();
  await page.locator('#reset-options').click();
  await page.keyboard.press('Escape');
  await expect(page.locator('#open-options')).toBeFocused();
  await page.locator('#obfuscate').click(); await expect(page.locator('#run-state')).toHaveText('Done');
  await page.locator('#source').fill(''); await page.locator('#obfuscate').click();
  await expect(page.locator('#error')).toContainText('Paste code');
});

test('local upload, safe filename, responsive layout and keyboard dialog', async ({ page }) => {
  await page.locator('#file-input').setInputFiles({ name: 'module.lua', mimeType: 'text/plain', buffer: Buffer.from("return 'local file'") });
  await expect(page.locator('#filename')).toHaveText('module.lua');
  await page.locator('#obfuscate').click(); await expect(page.locator('#run-state')).toHaveText('Done');
  const pending = page.waitForEvent('download'); await page.locator('#download').click();
  expect((await pending).suggestedFilename()).toBe('module.obf.luau');
  for (const width of [320, 768, 1024, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await expect(page.locator('#obfuscate')).toBeVisible();
    await page.locator('#open-options').click();
    await expect(page.locator('#options-dialog')).toBeVisible();
    await page.keyboard.press('Escape');
  }
});

test('cancel interrupts a real job and the next job works', async ({ page }) => {
  await page.locator('#preset').selectOption('hardened');
  await page.locator('#source').fill(Array.from({ length: 500 }, (_, i) => `print('entry ${i} with a long string for a cancellable build')`).join('\n'));
  await page.locator('#obfuscate').click(); await page.locator('#cancel').click();
  await expect(page.locator('#notice')).toContainText('cancelled');
  await expect(page.locator('#download')).toBeDisabled();
  await expect(page.locator('#obfuscate')).toBeEnabled();
  await page.locator('#source').fill('return 42'); await page.locator('#obfuscate').click();
  await expect(page.locator('#run-state')).toHaveText('Done');
});

test('native fallbacks are visible and strict coverage rejects them', async ({ page }) => {
  await page.locator('#source').fill('local function f()return getfenv()end;print(f())');
  await page.locator('#preset').selectOption('vm');
  await page.locator('#obfuscate').click();
  await expect(page.locator('#run-state')).toHaveText('Done');
  await expect(page.locator('#warnings')).toBeVisible();
  await expect(page.locator('#warnings')).toContainText('native');
  await page.locator('#preset').selectOption('hardened');
  await page.locator('#obfuscate').click();
  await expect(page.locator('#error')).toBeVisible();
  await expect(page.locator('#download')).toBeDisabled();
});

test('editor and settings have no automated WCAG AA violations', async ({ page }) => {
  let audit = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(audit.violations).toEqual([]);
  await page.locator('#open-options').click();
  audit = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
  expect(audit.violations).toEqual([]);
});

test('pretty output scrolls inside the editor instead of expanding the page', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.locator('#preset').selectOption('hardened');
  await page.locator('#pretty').check();
  await page.locator('#obfuscate').click();
  await expect(page.locator('#run-state')).toHaveText('Done');
  const bounds = await page.locator('#output-frame').boundingBox();
  expect(bounds.height).toBeLessThan(900);
  expect(await page.evaluate(() => document.documentElement.scrollHeight)).toBeLessThan(1200);
});
