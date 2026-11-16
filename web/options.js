// The controls mirror the public Rust configuration. Validation belongs to Rust.
const toggle = (key, label, help = '') => ({ key, label, help, type: 'checkbox' });
const number = (key, label, min, max, help = '') => ({ key, label, min, max, help, type: 'number' });
const select = (key, label, values, help = '') => ({ key, label, values, help, type: 'select' });
export const groups = [
  { title: 'Transformations', fields: [
    toggle('rename_identifiers', 'Renommer les variables locales'),
    toggle('encrypt_strings', 'Encoder les chaînes'),
    toggle('split_strings', 'Fragmenter les longues chaînes', 'Appliqué aux chaînes d’au moins 32 octets, lorsque leur encodage est activé.'),
    toggle('obfuscate_numbers', 'Transformer les entiers'),
    toggle('table_indirection', 'Masquer les accès aux propriétés'),
    toggle('control_flow_flatten', 'Aplatir le flux de contrôle'),
    toggle('inject_dead_code', 'Ajouter des blocs leurres'),
    toggle('anti_tamper', 'Vérifier l’intégrité des données', 'Détecte certaines modifications des données encodées. Les contrôles restent modifiables.'),
  ] },
  { title: 'Machine virtuelle', fields: [
    toggle('virtualize', 'Activer la virtualisation'),
    toggle('virtualize_chunk', 'Virtualiser le bloc principal'),
    select('vm_hardening', 'Diversification VM', [['basic', 'Basic'], ['client-max', 'Client max'], ['max', 'Max']]),
    toggle('vm_scatter_data', 'Disperser les tables VM', 'Augmente la taille et le coût de chargement.'),
    toggle('vm_bind_strings', 'Lier les chaînes aux prototypes'),
    toggle('vm_pack_bytecode', 'Compacter le bytecode'),
    toggle('vm_pack_constants', 'Compacter les constantes'),
    toggle('vm_predecode_bytecode', 'Conserver les instructions décodées', 'Plus rapide à l’exécution, avec un cache des instructions en clair.'),
    toggle('require_function_vm', 'Refuser les fonctions non virtualisées'),
    toggle('require_chunk_vm', 'Exiger la virtualisation du bloc principal'),
    toggle('require_bound_strings', 'Exiger toutes les chaînes liées', 'Nécessite l’intégrité, la liaison et la virtualisation du bloc principal.'),
  ] },
  { title: 'Chaînes et identifiants', fields: [
    select('string_decode_mode', 'Décodage des chaînes', [['lazy', 'À la demande'], ['eager', 'Au chargement']]),
    toggle('cache_strings', 'Conserver les chaînes décodées', 'Accélère les accès répétés en conservant les valeurs en clair.'),
    number('name_min_length', 'Longueur minimale des noms', 1, 128),
    number('name_max_length', 'Longueur maximale des noms', 1, 128),
  ] },
  { title: 'Flux de contrôle et nombres', fields: [
    number('min_blocks_for_cff', 'Seuil d’aplatissement', 2, 128),
    select('dead_code_density', 'Densité des leurres', [['low', 'Faible'], ['medium', 'Moyenne'], ['high', 'Élevée']]),
    number('fake_function_count', 'Nombre de fonctions leurres', 0, 64),
    number('mba_depth', 'Profondeur des expressions numériques', 1, 3),
    toggle('skip_trivial_numbers', 'Conserver les nombres 0 et 1'),
    toggle('opaque_predicates', 'Ajouter des prédicats arithmétiques', 'Option historique : bruit facilement simplifiable, désactivé dans les profils.'),
  ] },
  { title: 'Sortie', fields: [
    toggle('strip_types', 'Retirer les annotations de types'),
    toggle('strip_comments', 'Retirer les commentaires'),
    toggle('wrap_in_iife', 'Envelopper dans une fonction', 'Ajoute une frame d’appel et nécessite le retrait des types.'),
    { key: 'build_id', label: 'Identifiant de build', type: 'text', help: 'Vide : identifiant automatique. Visible dans l’en-tête du résultat.' },
  ] },
];
export const fields = groups.flatMap(group => group.fields);
export const descriptions = {
  rename: 'Renommage des variables locales et minification, sans encodage des chaînes.',
  lightweight: 'Renommage et encodage des chaînes, avec un faible surcoût.',
  native: 'Chaînes fragmentées, entiers transformés et flux de contrôle aplati.',
  vm: 'Compilation en bytecode privé et diversification. Surcoût de taille et d’exécution plus élevé.',
  hardened: 'VM maximale, chaînes liées et couverture stricte. Les constructions non couvertes sont refusées.',
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
    vm_bind_strings: hardened, require_bound_strings: hardened, require_function_vm: hardened,
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
    if (!input.reportValidity()) throw new Error(`Réglage invalide : ${field.label}`);
    const value = field.type === 'checkbox' ? input.checked : field.type === 'number' ? input.valueAsNumber : input.value;
    if (field.key !== 'build_id' || value.trim()) result[field.key] = value;
  }
  return result;
}
