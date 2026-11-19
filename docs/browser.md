# Browser playground and GitHub Pages

The browser app uses the same Rust library as the CLI, compiled to WebAssembly.
Source, settings and output stay in the browser. There is no obfuscation API,
analytics, third-party runtime CDN, account, or GitHub token in the page. The app
does not execute input or output scripts and does not persist source in storage.

## Interface

The homepage introduces RealObfuscator with a responsive purple hero, a locally
generated decorative globe and an overview of the available transformations.
"Open obfuscator" and "Obfuscate a script" lead to the dedicated workspace.
The globe respects reduced-motion preferences and pauses when off screen.
All visual assets, styles and scripts are served locally.

The static website has eight pages: home, features, obfuscator, script
identification, documentation, FAQ, support, and privacy/license information.
Shared navigation and footers are rendered by `scripts/build-pages.mjs` from
`web/site-layout.mjs` and the HTML content in `web/pages/`. URLs use relative
`.html` paths, including on a GitHub Pages repository subpath.

The color-theme selector offers Light, Dark, and System. `web/theme.js` applies
the saved preference before the styles load and responds to device-theme changes
in System mode. Only the theme preference is stored locally, never source code.
FAQ and documentation search filter local content. The support helper prepares
a GitHub issue link for user review; it never sends or submits a report.

Paste source or load a `.lua` / `.luau` file, select a profile and generate the
output. Advanced controls expose the public transformation, VM, string, naming,
formatting and coverage settings. A seed is optional and is passed as decimal text
to preserve all unsigned 64-bit values. The target runtime is Luau / Roblox.

The interface supports output copying, `.obf.luau` downloads, applied statistics,
native fallback warnings, a JSON report, error recovery and cancellation. Editing
the source or settings invalidates stale output. Default profiles match the CLI.
An isolated Web Worker handles generation, and terminating that worker cancels a
job. Source is limited to 1 MiB and jobs to 120 seconds; large or deeply nested
scripts can still exceed browser memory/stack limits. Use the native CLI for them.

File uploads use strict UTF-8 decoding. Files containing binary string bytes are
converted locally by the Rust worker to editable decimal escapes, preserving
their values instead of substituting replacement characters. Invalid bytes in
executable syntax produce an error. Editing the source discards pending imports.

The browser bridge reserves a 32 MiB Wasm stack instead of native stack switching.
Browser randomness comes from Web Crypto through `getrandom`. Native builds retain
their existing OS randomness and stack-growth path.

Obfuscation increases analysis effort but does not establish client-side secrecy;
the [static extraction audit](string-audit.md) remains applicable to browser output.

## Build and preview

Requirements: stable Rust, Node.js 22 or newer, the `wasm32-unknown-unknown` target,
and a Clang installation with WebAssembly support. Some minimal Windows Clang
distributions only support x86 and cannot compile the C dependency.

```sh
rustup target add wasm32-unknown-unknown
npm ci
npm run build:web
npm run preview:web
```

Open `http://127.0.0.1:4173`. Generated files are in `web/dist` and `web/pkg`, both
ignored by Git. Serve through HTTP; ES modules and Workers do not work from
`file://`. Resource paths are relative and support a repository Pages subpath.

For a Windows WASI SDK compiler, set `CC_wasm32_unknown_unknown` to its `clang.exe`
and `AR_wasm32_unknown_unknown` to its `llvm-ar.exe` before building. No SDK is
vendored. Linux CI installs normal Clang/LLVM from the runner's package manager.

## Verify

```sh
cargo test -p realobfuscator-web --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
npx playwright install chromium
npm run test:web
```

Browser tests call the actual Wasm engine for every preset, check deterministic
64-bit seeds, invalid source/settings, output downloads, local file input, stale
results, cancellation, native warnings and responsive layouts. On Windows, tests
use an existing Edge installation if present. When the local official Luau test
interpreter is available, the browser-generated hardened script is also executed
and its output checked. It is not executed by the website.

Hardened enables instruction-linked literal reconstruction by default, with an
explicit advanced-setting opt-out. Its JSON report includes
`vm_literal_instructions`. Browser tests check both settings using the actual
Wasm engine and execute generated binary-literal output in the official Luau CLI
when the interpreter is installed.

## Publish

`.github/workflows/pages.yml` runs on pushes to `main`, pull requests and manual
dispatch. It validates the Rust bridge, builds the Wasm engine and site, runs real
browser tests, then uploads and deploys the static artifact with the official
Pages actions. Pull requests only build/test and never deploy. No private source
used in the website is committed to GitHub or sent to this workflow.

Enable **Settings → Pages → Build and deployment → Source → GitHub Actions** on
the repository. The default URL is
`https://linkoro57.github.io/RealObfuscator/`. Deployment uses the `github-pages`
environment and repository `GITHUB_TOKEN` with `pages: write` and `id-token: write`
limited to the deployment job; the build job only reads repository contents.

This deployment process follows the official
[custom Pages workflow documentation](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).
