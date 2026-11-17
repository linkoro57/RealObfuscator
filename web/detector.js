// A separate, short-lived worker keeps scans independent of obfuscation jobs.
const $ = id => document.getElementById(id);
const dialog = $('detector-dialog'), input = $('detect-source'), status = $('detect-status');
const limit = 8 * 1024 * 1024, encoder = new TextEncoder();
let worker = null, timeout, version = 0;
const results = {
  matching_watermark: ['RealObfuscator watermark recognized', 'The embedded watermark matches this script after comment and formatting normalization. It is public and can be forged; this does not authenticate its author or origin.'],
  modified_watermark: ['RealObfuscator watermark found — content does not match', 'An embedded marker was recognized, but the script does not match its recorded content. It may have been edited or the marker copied.'],
  header_only: ['RealObfuscator header found', 'Only the visible attribution header was recognized. This can be an older output or a copied header; the origin is unverified.'],
  not_recognized: ['No recognized RealObfuscator marker', 'This does not exclude an older output or a script whose watermark was removed or transformed.'],
};
function stop() {
  clearTimeout(timeout);
  worker?.terminate(); worker = null;
  $('detect-run').disabled = false; $('detect-cancel').hidden = true;
  dialog.setAttribute('aria-busy', 'false');
}
function show(title, description, error = false) {
  delete status.dataset.result;
  status.hidden = false; status.classList.toggle('error', error);
  $('detect-title').textContent = title; $('detect-detail').textContent = description;
}
function invalidate() {
  version++; stop(); status.hidden = true;
}
$('open-detector').addEventListener('click', () => { dialog.showModal(); input.focus(); });
dialog.addEventListener('close', () => { version++; stop(); });
input.addEventListener('input', invalidate);
$('detect-cancel').addEventListener('click', () => { version++; stop(); show('Scan cancelled', 'The script remains in the detector.'); });
$('detect-run').addEventListener('click', () => {
  stop();
  const source = input.value;
  if (!source.trim()) { show('Add a script', 'Paste Lua/Luau code or load a file before checking.', true); input.focus(); return; }
  if (encoder.encode(source).length > limit) { show('File too large', 'The detector accepts up to 8 MiB of UTF-8 source.', true); return; }
  show('Checking for attribution markers…', 'The script is parsed locally and is never executed or uploaded.');
  $('detect-run').disabled = true; $('detect-cancel').hidden = false;
  dialog.setAttribute('aria-busy', 'true');
  const current = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  worker = current;
  timeout = setTimeout(() => { if (worker === current) { stop(); show('Scan timed out', 'Try a smaller script. No script was executed.', true); } }, 120000);
  current.onmessage = ({data}) => {
    if (worker !== current) return;
    if (data.type === 'ready') { current.postMessage({ type: 'identify', source }); return; }
    if (data.type === 'identification') {
      stop();
      const result = results[data.result.status];
      if (!result) { show('Unrecognized detector response', 'Reload the page and try again.', true); return; }
      show(...result); status.dataset.result = data.result.status;
    } else if (data.type === 'identification-error' || data.type === 'init-error') {
      stop(); show('Could not check this script', data.error, true);
    }
  };
  current.onerror = () => { if (worker === current) { stop(); show('Detector stopped', 'Reload the page or try a smaller script.', true); } };
});
$('detect-load').addEventListener('click', () => $('detect-file').click());
$('detect-file').addEventListener('change', async event => {
  const file = event.target.files[0]; event.target.value = '';
  if (!file) return;
  invalidate(); const request = version;
  if (file.size > limit) { show('File too large', 'The detector accepts files up to 8 MiB.', true); return; }
  try {
    const bytes = await file.arrayBuffer();
    if (version !== request) return;
    input.value = new TextDecoder('utf-8', {fatal:true}).decode(bytes);
    show('File loaded', `${file.name} — select Check script to inspect its markers.`);
  } catch { if (version === request) show('Could not read this file', 'Choose a UTF-8 Lua/Luau source file.', true); }
});
