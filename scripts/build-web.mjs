import { spawnSync } from 'node:child_process';
import { mkdir, copyFile, readdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const build = spawnSync(process.execPath, ['node_modules/wasm-pack/run.js', 'build', 'web/engine', '--target', 'web', '--release', '--out-dir', '../pkg', '--out-name', 'engine', '--locked'], { stdio: 'inherit' });
if (build.error) throw build.error;
if (build.status !== 0) process.exit(build.status ?? 1);
const dist = resolve('web/dist');
await mkdir(`${dist}/pkg`, { recursive: true });
for (const file of ['index.html', 'styles.css', 'app.js', 'options.js', 'worker.js', 'icon.svg']) await copyFile(`web/${file}`, `${dist}/${file}`);
for (const file of await readdir('web/pkg')) if (file.endsWith('.wasm') || file.endsWith('.js')) await copyFile(`web/pkg/${file}`, `${dist}/pkg/${file}`);
await copyFile('LICENSE', `${dist}/LICENSE`);
await writeFile(`${dist}/.nojekyll`, '');
console.log(`Static site ready: ${dist}`);
