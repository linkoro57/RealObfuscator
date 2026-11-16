// The controls mirror the public Rust configuration. Validation belongs to Rust.
const toggle = (key, label, help = '') => ({ key, label, help, type: 'checkbox' });
const number = (key, label, min, max, help = '') => ({ key, label, min, max, help, type: 'number' });
const select = (key, label, values, help = '') => ({ key, label, values, help, type: 'select' });
export const groups = [
  { title: 'Transformations', fields: [
    toggle('rename_identifiers', 'Rename local bindings'),
    toggle('encrypt_strings', 'Encode strings'),
    toggle('split_strings', 'Split long strings', 'Applied to strings of at least 32 bytes when string encoding is enabled.'),
    toggle('obfuscate_numbers', 'Transform integers'),
    toggle('table_indirection', 'Hide property access'),
    toggle('control_flow_flatten', 'Flatten control flow'),
    toggle('inject_dead_code', 'Add decoy blocks'),
    toggle('anti_tamper', 'Check data integrity', 'Detects some changes to encoded data. The checks can still be modified.'),
  ] },
  { title: 'Virtual machine', fields: [
    toggle('virtualize', 'Enable virtualization'),
    toggle('virtualize_chunk', 'Virtualize the main chunk'),
    select('vm_hardening', 'VM diversification', [['basic', 'Basic'], ['client-max', 'Client max'], ['max', 'Max']]),
    toggle('vm_scatter_data', 'Scatter VM tables', 'Increases output size and load cost.'),
    toggle('vm_bind_strings', 'Bind strings to prototypes'),
    toggle('vm_literal_programs', 'Reconstruct strings through VM instructions', 'Replaces standalone literal payloads with stateful, shuffled recipes. Requires bound strings and integrity checks; increases size and runtime cost.'),
    toggle('vm_pack_bytecode', 'Pack bytecode'),
    toggle('vm_pack_constants', 'Pack constants'),
    toggle('vm_predecode_bytecode', 'Cache decoded instructions', 'Faster execution with a retained plaintext instruction cache.'),
    toggle('require_function_vm', 'Reject native function fallbacks'),
    toggle('require_chunk_vm', 'Require main-chunk virtualization'),
    toggle('require_bound_strings', 'Require all strings to be bound', 'Requires integrity checks, string binding and main-chunk virtualization.'),
  ] },
  { title: 'Strings and identifiers', fields: [
    select('string_decode_mode', 'String decoding', [['lazy', 'On demand'], ['eager', 'At startup']]),
    toggle('cache_strings', 'Cache decoded strings', 'Speeds up repeated access by keeping plaintext values.'),
    number('name_min_length', 'Minimum name length', 1, 128),
    number('name_max_length', 'Maximum name length', 1, 128),
  ] },
  { title: 'Control flow and numbers', fields: [
    number('min_blocks_for_cff', 'Flattening threshold', 2, 128),
    select('dead_code_density', 'Decoy density', [['low', 'Low'], ['medium', 'Medium'], ['high', 'High']]),
    number('fake_function_count', 'Decoy function count', 0, 64),
    number('mba_depth', 'Numeric expression depth', 1, 3),
    toggle('skip_trivial_numbers', 'Keep the numbers 0 and 1'),
    toggle('opaque_predicates', 'Add arithmetic predicates', 'Legacy option: easily simplified noise, disabled by presets.'),
  ] },
  { title: 'Output', fields: [
    toggle('strip_types', 'Strip type annotations'),
    toggle('strip_comments', 'Strip comments'),
    toggle('wrap_in_iife', 'Wrap in a function', 'Isolates source locals from helpers. Adds a call frame and requires type stripping.'),
    { key: 'build_id', label: 'Build identifier', type: 'text', help: 'Empty: automatic identifier. Visible in the output header.' },
  ] },
];
export const fields = groups.flatMap(group => group.fields);
export const descriptions = {
  rename: 'Local renaming and minification without string encoding.',
  lightweight: 'Local renaming and string encoding with low overhead.',
  native: 'Split strings, transformed integers and flattened control flow.',
  vm: 'Private bytecode compilation and diversification. Higher size and runtime overhead.',
  hardened: 'Maximum VM settings, instruction-linked strings and strict coverage. Higher size and execution cost; unsupported constructs are rejected.',
};
export function defaults(preset) {
  const native = ['native', 'vm', 'hardened'].includes(preset);
  const vm = ['vm', 'hardened'].includes(preset);
  const hardened = preset === 'hardened';
  return {
    rename_identifiers: true, encrypt_strings: preset !== 'rename', split_strings: native,
    cache_strings: !vm, obfuscate_numbers: native, control_flow_flatten: native,
    opaque_predicates: false, inject_dead_code: native, table_indirection: native,
    anti_tamper: native, virtualize: vm, virtualize_chunk: true, vm_scatter_data: vm,
    vm_bind_strings: hardened, vm_literal_programs: hardened, require_bound_strings: hardened, require_function_vm: hardened,
    require_chunk_vm: hardened, name_min_length: 8, name_max_length: 14,
    string_decode_mode: native ? 'lazy' : 'eager', mba_depth: 2, skip_trivial_numbers: true,
    min_blocks_for_cff: 3, dead_code_density: 'medium', fake_function_count: 5,
    wrap_in_iife: false, vm_hardening: vm ? 'max' : 'basic', vm_pack_bytecode: vm,
    vm_pack_constants: vm, vm_predecode_bytecode: false, strip_comments: true,
    strip_types: true, build_id: '',
  };
}
export function renderOptions(container) {
  for (const group of groups) {
    const fieldset = document.createElement('fieldset'); fieldset.className = 'option-group';
    const legend = document.createElement('legend'); legend.textContent = group.title; fieldset.append(legend);
    for (const field of group.fields) {
      const row = document.createElement('div'); row.className = 'option';
      const label = document.createElement('label'); label.htmlFor = `option-${field.key}`; label.textContent = field.label;
      const input = document.createElement(field.type === 'select' ? 'select' : 'input');
      input.id = label.htmlFor; input.dataset.option = field.key;
      if (field.type === 'select') for (const [value, text] of field.values) input.add(new Option(text, value));
      else input.type = field.type;
      if (field.type === 'number') { input.min = field.min; input.max = field.max; input.step = '1'; input.required = true; }
      if (field.help) { const help = document.createElement('small'); help.id = `${input.id}-help`; help.textContent = field.help; label.append(help); input.setAttribute('aria-describedby', help.id); }
      row.append(label, input); fieldset.append(row);
    }
    container.append(fieldset);
  }
}
export function setOptions(preset) {
  const values = defaults(preset);
  for (const field of fields) {
    const input = document.getElementById(`option-${field.key}`);
    if (field.type === 'checkbox') input.checked = values[field.key]; else input.value = values[field.key];
  }
  syncDependencies(false);
}
export function syncDependencies(clear = true) {
  const vm = document.getElementById('option-virtualize').checked;
  const vmFields = groups[1].fields.filter(field => field.key !== 'virtualize');
  for (const field of vmFields) {
    const input = document.getElementById(`option-${field.key}`);
    input.disabled = !vm;
    input.closest('.option').classList.toggle('disabled', !vm);
    if (!vm && clear && field.type === 'checkbox') input.checked = false;
  }
}
export function settings(preset, seed, pretty) {
  const result = { preset, seed, minify: !pretty };
  for (const field of fields) {
    const input = document.getElementById(`option-${field.key}`);
    if (!input.reportValidity()) throw new Error(`Invalid setting : ${field.label}`);
    const value = field.type === 'checkbox' ? input.checked : field.type === 'number' ? input.valueAsNumber : input.value;
    if (field.key !== 'build_id' || value.trim()) result[field.key] = value;
  }
  return result;
}
