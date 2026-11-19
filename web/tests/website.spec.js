import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

const routes = ['index', 'features', 'obfuscate', 'identify', 'docs', 'faq', 'support', 'legal'];

test('themes persist across real page navigation and System follows the device', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.getByLabel('Color theme').selectOption('dark');
  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: 'Documentation', exact: true }).click();
  await expect(page).toHaveURL(/docs\.html$/);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(page.getByLabel('Color theme')).toHaveValue('dark');
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.getByLabel('Color theme').selectOption('system');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
});

test('all pages render, fit supported widths and pass accessibility in both themes', async ({ page }) => {
  test.setTimeout(120000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  for (const route of routes) {
    const response = await page.goto(`/${route}.html`);
    expect(response.status()).toBe(200);
    await expect(page.locator('h1')).toHaveCount(1);
    for (const width of [320, 768, 1024, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      expect.soft(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${route} at ${width}px`).toBe(true);
    }
    for (const theme of ['light', 'dark']) {
      await page.getByLabel('Color theme').selectOption(theme);
      const result = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21aa']).analyze();
      expect.soft(result.violations, `${route}, ${theme}`).toEqual([]);
    }
  }
  expect(errors).toEqual([]);
});

test('mobile navigation opens, supports Escape and follows links', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  const menu = page.getByRole('button', { name: 'Menu' });
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeHidden();
  await menu.click();
  await expect(menu).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('Escape');
  await expect(menu).toBeFocused();
  await expect(menu).toHaveAttribute('aria-expanded', 'false');
  await menu.click();
  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: 'FAQ', exact: true }).click();
  await expect(page).toHaveURL(/faq\.html$/);
});

test('FAQ and documentation search handle matches, empty results and topic navigation', async ({ page }) => {
  await page.goto('/faq.html');
  await page.getByLabel('Find an answer').fill('seed');
  await expect(page.locator('#faq-list details:visible')).toHaveCount(1);
  await expect(page.locator('#faq-list details:visible')).toHaveAttribute('open', '');
  await page.getByLabel('Find an answer').fill('no-such-topic-981');
  await expect(page.locator('#faq-search-status')).toContainText('No matches');
  await page.getByLabel('Find an answer').clear();
  await expect(page.locator('#faq-list details:visible')).toHaveCount(12);
  await page.goto('/docs.html');
  await page.getByLabel('Find a topic').fill('command line');
  await expect(page.locator('#cli')).toBeVisible();
  await expect(page.locator('#quick-start')).toBeHidden();
  await page.getByRole('navigation', { name: 'Topics' }).getByRole('link', { name: 'Quick start', exact: true }).click();
  await expect(page.locator('#quick-start')).toBeVisible();
  await expect(page.getByLabel('Find a topic')).toHaveValue('');
});

test('support prepares a report without sending and standalone detector works', async ({ page }) => {
  await page.goto('/support.html');
  await page.getByLabel('Short description').fill('Example & reproduction');
  await page.getByLabel('Steps, expected result and actual result').fill('Expected: 42\nActual: error');
  await page.getByRole('button', { name: 'Prepare GitHub issue' }).click();
  const url = new URL(await page.getByRole('link', { name: 'Review on GitHub' }).getAttribute('href'));
  expect(url.hostname).toBe('github.com');
  expect(url.searchParams.get('title')).toBe('Example & reproduction');
  expect(url.searchParams.get('body')).toContain('Expected: 42\nActual: error');
  await page.getByLabel('Short description').fill('Changed');
  await expect(page.locator('#issue-result')).toBeHidden();
  await page.goto('/identify.html');
  await page.getByRole('button', { name: 'Check script' }).click();
  await expect(page.locator('#detect-title')).toHaveText('Add a script');
  await page.getByLabel('Script to inspect').fill('print(42)');
  await page.getByRole('button', { name: 'Check script' }).click();
  await expect(page.locator('#detect-status')).toHaveAttribute('data-result', 'not_recognized');
});
