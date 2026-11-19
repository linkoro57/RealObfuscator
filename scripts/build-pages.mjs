import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pages, renderPage } from '../web/site-layout.mjs';

export async function buildPages() {
  const destination = resolve('web/dist');
  await mkdir(destination, { recursive: true });
  for (const page of pages) {
    const content = await readFile(`web/pages/${page.source || page.file}.html`, 'utf8');
    await writeFile(resolve(destination, `${page.file}.html`), renderPage(page, content));
  }
  console.log(`Rendered ${pages.length} pages.`);
}
if (process.argv[1] && resolve(process.argv[1]) === resolve('scripts/build-pages.mjs')) await buildPages();
