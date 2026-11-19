// Runs before styles are loaded so a saved theme does not flash on navigation.
(() => {
  const key = 'realobfuscator-theme';
  const system = matchMedia('(prefers-color-scheme: dark)');
  const valid = value => ['light', 'dark', 'system'].includes(value);
  let choice = 'system';
  try { const saved = localStorage.getItem(key); if (valid(saved)) choice = saved; } catch { /* Storage can be unavailable. */ }
  function apply() {
    document.documentElement.dataset.theme = choice === 'system' ? (system.matches ? 'dark' : 'light') : choice;
    const control = document.getElementById('theme-select');
    if (control) control.value = choice;
  }
  apply();
  system.addEventListener('change', apply);
  addEventListener('storage', event => {
    if (event.key === key) { choice = valid(event.newValue) ? event.newValue : 'system'; apply(); }
  });
  document.addEventListener('DOMContentLoaded', () => {
    apply();
    document.getElementById('theme-select')?.addEventListener('change', event => {
      choice = event.target.value;
      try { localStorage.setItem(key, choice); } catch { /* Keep the selection for this page. */ }
      apply();
    });
  });
})();
