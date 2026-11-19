const repository = 'https://github.com/linkoro57/RealObfuscator';
export const pages = [
  { file: 'index', source: 'home', title: 'Lua & Luau Obfuscation', description: 'Transform your Lua and Luau scripts locally with RealObfuscator.', label: 'Home' },
  { file: 'features', title: 'Features', description: 'Explore virtualization, code transformation and RealObfuscator presets.', label: 'Features' },
  { file: 'obfuscate', title: 'Obfuscator', description: 'Obfuscate Lua and Luau in your browser. No account or upload required.', label: 'Obfuscator', script: 'app' },
  { file: 'identify', title: 'Script identification', description: 'Check RealObfuscator attribution markers locally.', label: 'Identify a script', script: 'detector' },
  { file: 'docs', title: 'Documentation', description: 'Get started with the browser tool, presets and the RealObfuscator CLI.', label: 'Documentation' },
  { file: 'faq', title: 'Frequently asked questions', description: 'Answers about compatibility, privacy, presets and obfuscation.', label: 'FAQ' },
  { file: 'support', title: 'Support', description: 'Troubleshoot RealObfuscator and prepare a useful bug report.', label: 'Support' },
  { file: 'legal', title: 'Privacy & license', description: 'Local processing, theme preferences, licensing and attribution.', label: 'Privacy & license' },
];
const brand = `<a class="site-brand" href="./index.html" aria-label="RealObfuscator home"><img src="./icon.svg" alt="" width="36" height="36"><span>Real<span class="brand-light">Obfuscator</span></span></a>`;
function link(page, active) {
  return `<a href="./${page.file}.html"${page.file === active ? ' aria-current="page"' : ''}>${page.label}</a>`;
}
export function renderPage(page, content) {
  const home = page.file === 'index';
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="description" content="${page.description}">
  <meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data:; connect-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'">
  <meta name="color-scheme" content="light dark">
  <meta name="theme-color" content="#563491">
  <title>RealObfuscator — ${page.title}</title>
  <link rel="icon" type="image/svg+xml" href="./icon.svg">
  <script src="./theme.js"></script>
  <link rel="stylesheet" href="./styles.css">
  <link rel="stylesheet" href="./website.css">
  <link rel="stylesheet" href="./pages.css">
  <script type="module" src="./website.js"></script>
  ${page.script ? `<script type="module" src="./${page.script}.js"></script>` : ''}
</head>
<body class="page-${page.file}">
  <a class="skip-link" href="#main">Skip to content</a>
  <header class="global-header${home ? ' on-hero' : ''}">
    <div class="site-header site-container">${brand}
      <button class="menu-toggle button" type="button" aria-controls="main-navigation" aria-expanded="false">Menu <span aria-hidden="true">☰</span></button>
      <nav id="main-navigation" class="site-nav" aria-label="Main navigation">
        ${['features', 'docs', 'faq', 'support'].map(file => link(pages.find(p => p.file === file), page.file)).join('')}
        <a class="site-button nav-cta" href="./obfuscate.html"${page.file === 'obfuscate' ? ' aria-current="page"' : ''}>Open obfuscator</a>
      </nav>
      <label class="theme-picker"><span class="sr-only">Color theme</span><select id="theme-select" aria-label="Color theme"><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></label>
    </div>
  </header>
  <main id="main" tabindex="-1">${content}</main>
  ${page.file !== 'obfuscate' ? `<section class="bottom-cta site-container" aria-labelledby="cta-heading"><div><p class="eyebrow">YOUR NEXT BUILD STARTS HERE</p><h2 id="cta-heading">A new shape for your next script.</h2><p>Free, open source, and running on your device.</p></div><a class="site-button purple" href="./obfuscate.html">Open obfuscator</a></section>` : ''}
  <footer class="global-footer"><div class="site-container footer-grid"><div>${brand}<p>Lua &amp; Luau obfuscation.<br>On your terms.</p><a href="${repository}" target="_blank" rel="noreferrer">View the source on GitHub</a></div><div><h2>Product</h2>${['index', 'features', 'obfuscate', 'identify'].map(file => link(pages.find(p => p.file === file), page.file)).join('')}</div><div><h2>Resources</h2>${['docs', 'faq', 'support'].map(file => link(pages.find(p => p.file === file), page.file)).join('')}<a href="${repository}/releases" target="_blank" rel="noreferrer">Releases</a></div><div><h2>Project</h2>${link(pages.find(p => p.file === 'legal'), page.file)}<a href="./LICENSE">GNU GPL v3</a><a href="./NOTICE.md">Attribution notices</a></div></div><div class="footer-base site-container"><span>RealObfuscator · Open source under GPL-3.0-only</span><span>Built with Rust &amp; WebAssembly</span></div></footer>
  <noscript><p class="callout site-container">Navigation and documentation work without JavaScript. Interactive tools and theme selection require JavaScript; the tools also require WebAssembly.</p></noscript>
</body>
</html>`;
}
