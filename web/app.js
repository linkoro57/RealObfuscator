import { fields, defaults, descriptions, renderOptions, setOptions, syncDependencies, settings } from './options.js';

const $ = id => document.getElementById(id);
const source = $('source'), output = $('output');
const example = 'local function greet(name)\n    return "Hello, " .. name .. "!"\nend\n\nprint(greet("Roblox"))\n';
const encoder = new TextEncoder();
let worker, ready = false, busy = false, job = 0, timer, result = null, filename = 'script.luau';
const size = value => value < 1024 ? `${value} byte${value === 1 ? '' : 's'}` : `${(value / 1024).toLocaleString('en-US', { maximumFractionDigits: 1 })} KiB`;

function lines(input, gutter) {
  const count = Math.min(input.value.split('\n').length, 10000);
  gutter.textContent = Array.from({ length: Math.max(1, count) }, (_, index) => index + 1).join('\n');
  gutter.scrollTop = input.scrollTop;
}
function message(text, error = false) {
  $('error').hidden = !error; $('notice').hidden = error;
  if (error) $('error').textContent = text; else $('notice').textContent = text;
}
function clearResult() {
  result = null; output.value = ''; lines(output, $('output-lines'));
  $('copy').disabled = $('download').disabled = true;
  $('summary').hidden = $('stats-details').hidden = $('download-report').hidden = $('warnings').hidden = true;
  $('output-size').textContent = '—'; $('output-label').textContent = 'No output yet';
  $('notice').classList.remove('success'); $('run-state').textContent = 'Idle';
}
function invalidate() {
  if (busy) cancel('The input changed. Run obfuscation again.');
  clearResult();
  message('Ready to obfuscate the current source and settings.');
}
function setBusy(value) {
  busy = value; $('working').hidden = !value; $('cancel').hidden = !value;
  $('obfuscate').hidden = value; $('obfuscate').disabled = !ready;
  $('output-frame').setAttribute('aria-busy', String(value));
  $('run-state').textContent = value ? 'Running' : 'Idle';
}
function createWorker() {
  ready = false; $('obfuscate').disabled = true; $('engine-status').textContent = 'Loading engine…';
  const current = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  worker = current;
  const initTimer = setTimeout(() => {
    if (!ready && worker === current) { current.terminate(); $('engine-status').textContent = 'Engine unavailable'; message('The engine could not load. Reload the page to try again.', true); }
  }, 45000);
  current.onmessage = ({ data }) => {
    if (worker !== current) return;
    if (data.type === 'ready') { clearTimeout(initTimer); ready = true; $('obfuscate').disabled = false; $('engine-status').textContent = 'Engine ready · Rust / WASM'; return; }
    if (data.type === 'init-error') { clearTimeout(initTimer); $('engine-status').textContent = 'Engine unavailable'; message(`Could not load the engine. Reload the page.\n${data.error}`, true); return; }
    if (data.id !== job) return;
    clearTimeout(timer); setBusy(false);
    if (data.type === 'error') {
      clearResult(); message(data.error, true); $('run-state').textContent = 'Error';
      if (data.fatal) { current.terminate(); createWorker(); }
      return;
    }
    result = data.result; output.value = result.code; lines(output, $('output-lines'));
    $('copy').disabled = $('download').disabled = false;
    $('output-size').textContent = size(result.report.output_bytes); $('output-label').textContent = outputName();
    $('run-state').textContent = 'Done'; $('notice').classList.add('success');
    message(`Done in ${(data.duration / 1000).toLocaleString('en-US', { maximumFractionDigits: 2 })} s. Your output is ready to download.`);
    renderReport(result.report, data.duration);
    $('warnings').replaceChildren(); $('warnings').hidden = !result.warnings.length;
    for (const text of result.warnings) { const p = document.createElement('p'); p.textContent = text; $('warnings').append(p); }
  };
  current.onerror = event => {
    if (worker !== current) return;
    clearTimeout(initTimer); clearTimeout(timer); ready = false; setBusy(false); clearResult();
    $('obfuscate').disabled = true; $('engine-status').textContent = 'Engine stopped';
    message(`The engine stopped. Reload the page or use the CLI for this script. ${event.message || ''}`, true);
  };
}
function cancel(text = 'Processing cancelled. Your source is preserved.') {
  if (!busy) return;
  clearTimeout(timer); job++; worker.terminate(); setBusy(false); clearResult(); message(text); createWorker();
}
function outputName() { return filename.replace(/(?:\.obf)?\.(?:lua|luau)$/i, '') + '.obf.luau'; }
function download(text, name, type = 'text/plain;charset=utf-8') {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const link = document.createElement('a'); link.href = url; link.download = name; document.body.append(link); link.click(); link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
function addStat(list, label, value) {
  const row = document.createElement('div'), dt = document.createElement('dt'), dd = document.createElement('dd');
  dt.textContent = label; dd.textContent = value; row.append(dt, dd); list.append(row);
}
function renderReport(report, duration) {
  $('summary').replaceChildren(); $('summary').hidden = false;
  for (const [label, value] of [ ['Source', size(report.input_bytes)], ['Output', size(report.output_bytes)], ['Size ratio', `×${(report.output_bytes / report.input_bytes).toFixed(2)}`], ['Time', `${Math.round(duration)} ms`], ['Encoded strings', report.encrypted_strings], ['VM functions', report.vm_functions], ['VM instructions', report.vm_instructions] ]) addStat($('summary'), label, value);
  $('stats').replaceChildren();
  for (const [key, value] of Object.entries(report)) addStat($('stats'), key, value && typeof value === 'object' ? JSON.stringify(value) : value ?? '—');
  $('stats-details').hidden = $('download-report').hidden = false;
}
function updateOptionsCount() {
  const base = defaults($('preset').value);
  let count = Number($('pretty').checked);
  for (const field of fields) {
    const input = $(`option-${field.key}`);
    const value = field.type === 'checkbox' ? input.checked : field.type === 'number' ? input.valueAsNumber : input.value;
    if (value !== base[field.key]) count++;
  }
  $('option-count').textContent = count; $('option-count').hidden = !count;
}
renderOptions($('options-fields')); setOptions($('preset').value);
$('preset').addEventListener('change', () => { setOptions($('preset').value); $('pretty').checked = false; $('preset-description').textContent = descriptions[$('preset').value]; updateOptionsCount(); invalidate(); });
$('options-fields').addEventListener('input', event => { if (event.target.id === 'option-virtualize') syncDependencies(); updateOptionsCount(); invalidate(); });
$('reset-options').addEventListener('click', () => { setOptions($('preset').value); $('pretty').checked = false; updateOptionsCount(); invalidate(); });
$('open-options').addEventListener('click', () => $('options-dialog').showModal());
$('close-options').addEventListener('click', () => $('options-dialog').close());
$('options-dialog').addEventListener('close', () => $('open-options').focus());
$('pretty').addEventListener('change', () => { updateOptionsCount(); invalidate(); });
$('seed').addEventListener('input', invalidate);
$('random-seed').addEventListener('click', () => { const words = crypto.getRandomValues(new Uint32Array(2)); $('seed').value = ((BigInt(words[0]) << 32n) | BigInt(words[1])).toString(); invalidate(); });
source.addEventListener('input', () => { $('source-size').textContent = size(encoder.encode(source.value).length); lines(source, $('source-lines')); invalidate(); });
source.addEventListener('scroll', () => { $('source-lines').scrollTop = source.scrollTop; });
output.addEventListener('scroll', () => { $('output-lines').scrollTop = output.scrollTop; });
$('example').addEventListener('click', () => { source.value = example; filename = 'example.luau'; $('filename').textContent = filename; source.dispatchEvent(new Event('input')); source.focus(); });
$('clear').addEventListener('click', () => { source.value = ''; filename = 'script.luau'; $('filename').textContent = filename; source.dispatchEvent(new Event('input')); source.focus(); });
$('load-file').addEventListener('click', () => $('file-input').click());
$('file-input').addEventListener('change', async event => {
  const file = event.target.files[0]; event.target.value = ''; if (!file) return;
  if (!/\.(lua|luau)$/i.test(file.name)) { message('Choose a .lua or .luau file.', true); return; }
  if (file.size > 1024 * 1024) { message('Browser limit: 1 MiB. Use the CLI for larger files.', true); return; }
  try { const text = await file.text(); filename = file.name; source.value = text; $('filename').textContent = filename; source.dispatchEvent(new Event('input')); }
  catch { message('Could not read this file.', true); }
});
$('configuration').addEventListener('submit', event => {
  event.preventDefault(); if (!ready || busy) return;
  if (!source.value.trim()) { message('Paste code or load a file before obfuscating.', true); source.focus(); return; }
  if (encoder.encode(source.value).length > 1024 * 1024) { message('Browser limit: 1 MiB. Use the CLI for this file.', true); return; }
  try {
    const options = settings($('preset').value, $('seed').value.trim(), $('pretty').checked);
    clearResult(); setBusy(true); message('Processing locally in your browser…');
    worker.postMessage({ type: 'obfuscate', id: ++job, source: source.value, settings: options });
    timer = setTimeout(() => cancel('The 120-second limit was reached. Try a lighter preset or use the CLI.'), 120000);
  } catch (error) { message(String(error), true); }
});
$('cancel').addEventListener('click', () => cancel());
$('copy').addEventListener('click', async () => {
  if (!result) return;
  try { await navigator.clipboard.writeText(result.code); message('Obfuscated source copied to the clipboard.'); }
  catch { output.focus(); output.select(); message('Output selected. Press Ctrl+C or Cmd+C to copy.'); }
});
$('download').addEventListener('click', () => { if (result) download(result.code, outputName()); });
$('download-report').addEventListener('click', () => { if (result) download(JSON.stringify({ report: result.report, warnings: result.warnings }, null, 2), outputName().replace('.luau', '.report.json'), 'application/json'); });
source.value = example; source.dispatchEvent(new Event('input'));
if (typeof Worker === 'undefined' || typeof WebAssembly === 'undefined') { message('This browser does not support the required Web Workers and WebAssembly.', true); $('engine-status').textContent = 'Unsupported browser'; }
else createWorker();
