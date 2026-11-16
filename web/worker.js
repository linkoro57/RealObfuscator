import init, { obfuscate } from './pkg/engine.js';

const ready = init();
ready.then(() => self.postMessage({ type: 'ready' })).catch(error => self.postMessage({ type: 'init-error', error: String(error) }));
self.onmessage = async ({ data }) => {
  if (data.type !== 'obfuscate') return;
  try {
    await ready;
    const start = performance.now();
    const result = JSON.parse(obfuscate(data.source, JSON.stringify(data.settings)));
    self.postMessage({ type: 'result', id: data.id, result, duration: performance.now() - start });
  } catch (error) {
    self.postMessage({ type: 'error', id: data.id, error: String(error), fatal: error instanceof WebAssembly.RuntimeError });
  }
};
