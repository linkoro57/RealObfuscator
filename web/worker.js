import init, { obfuscate, decode_source, identify } from './pkg/engine.js';

const ready = init();
ready.then(() => self.postMessage({ type: 'ready' })).catch(error => self.postMessage({ type: 'init-error', error: String(error) }));
self.onmessage = async ({ data }) => {
  if (data.type === 'identify') {
    try { await ready; self.postMessage({ type: 'identification', result: JSON.parse(identify(data.source)) }); }
    catch (error) { self.postMessage({ type: 'identification-error', error: String(error) }); }
    return;
  }
  if (data.type === 'decode-source') {
    try { await ready; self.postMessage({type:'source-decoded',id:data.id,source:decode_source(data.bytes)}); }
    catch(error) {self.postMessage({type:'source-error',id:data.id,error:String(error)});}
    return;
  }
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
