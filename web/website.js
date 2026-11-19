const header = document.querySelector('.global-header');
const menu = document.querySelector('.menu-toggle');
if (header && menu) {
  header.classList.add('menu-ready');
  const closeMenu = () => { header.classList.remove('menu-open'); menu.setAttribute('aria-expanded', 'false'); };
  menu.addEventListener('click', () => {
    const expanded = header.classList.toggle('menu-open');
    menu.setAttribute('aria-expanded', String(expanded));
  });
  header.addEventListener('keydown', event => {
    if (event.key === 'Escape' && header.classList.contains('menu-open')) { closeMenu(); menu.focus(); }
  });
  matchMedia('(max-width: 900px)').addEventListener('change', closeMenu);
}

function setupSearch(inputId, containerId, statusId) {
  const input = document.getElementById(inputId);
  if (!input) return;
  const items = [...document.querySelectorAll(`#${containerId} [data-search-item]`)];
  const status = document.getElementById(statusId);
  input.addEventListener('input', () => {
    const query = input.value.trim().toLocaleLowerCase();
    const terms = query.split(/\s+/).filter(Boolean);
    let count = 0;
    for (const item of items) {
      const matches = terms.every(term => item.textContent.toLocaleLowerCase().includes(term));
      item.hidden = !matches;
      if (matches) count++;
      if (item.tagName === 'DETAILS') item.open = Boolean(query && matches);
    }
    status.hidden = !query;
    status.textContent = count ? `${count} matching ${count === 1 ? 'topic' : 'topics'}.` : 'No matches. Try another search or contact support.';
  });
  document.querySelectorAll('.docs-sidebar nav a').forEach(link => link.addEventListener('click', () => {
    input.value = ''; input.dispatchEvent(new Event('input'));
  }));
}
setupSearch('faq-search', 'faq-list', 'faq-search-status');
setupSearch('docs-search', 'docs-content', 'docs-search-status');

document.querySelectorAll('.copy-snippet').forEach(button => button.addEventListener('click', async () => {
  const code = button.parentElement.querySelector('code');
  try {
    await navigator.clipboard.writeText(code.textContent);
    button.textContent = 'Copied';
    setTimeout(() => { button.textContent = 'Copy command'; }, 2000);
  } catch {
    const selection = getSelection();
    const range = document.createRange(); range.selectNodeContents(code);
    selection.removeAllRanges(); selection.addRange(range);
    button.textContent = 'Select text and copy';
  }
}));

const issueForm = document.getElementById('issue-form');
issueForm?.addEventListener('submit', event => {
  event.preventDefault();
  const title = document.getElementById('issue-title').value.trim();
  const details = document.getElementById('issue-details').value.trim();
  if (!title || !details) return;
  const preset = document.getElementById('issue-preset').value;
  const url = new URL('https://github.com/linkoro57/RealObfuscator/issues/new');
  url.searchParams.set('title', title);
  url.searchParams.set('body', `Preset / tool: ${preset}\n\n${details}`);
  document.getElementById('issue-link').href = url.href;
  document.getElementById('issue-result').hidden = false;
});
issueForm?.addEventListener('input', () => { document.getElementById('issue-result').hidden = true; });

// Decorative globe, generated locally without external assets or requests.
const canvas = document.getElementById('protection-globe');
const context = canvas?.getContext('2d');
if (context) {
  const motion = matchMedia('(prefers-reduced-motion: reduce)');
  const points = Array.from({ length: 2200 }, (_, index) => {
    const y = 1 - (index / 2199) * 2;
    const radius = Math.sqrt(1 - y * y);
    const angle = index * Math.PI * (3 - Math.sqrt(5));
    return [Math.cos(angle) * radius, y, Math.sin(angle) * radius];
  });
  let frame = 0, visible = true, size = 0, animation;
  function draw() {
    const ratio = Math.min(devicePixelRatio || 1, 2);
    const nextSize = canvas.clientWidth;
    if (size !== nextSize) {
      size = nextSize;
      canvas.width = size * ratio;
      canvas.height = size * ratio;
    }
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, size, size);
    const rotation = frame * .0012 + .3;
    const cos = Math.cos(rotation), sin = Math.sin(rotation);
    for (let index = 0; index < points.length; index++) {
      const [x, y, z] = points[index];
      const rx = x * cos - z * sin, rz = x * sin + z * cos;
      const wave = 1 + .045 * Math.sin(y * 13 + rotation * 2) + .035 * Math.cos(rx * 11);
      const perspective = 2.8 / (2.8 - rz * .35);
      const px = size / 2 + rx * size * .365 * wave * perspective;
      const py = size / 2 + y * size * .365 * wave * perspective;
      context.beginPath();
      context.fillStyle = `rgba(233, 178, 255, ${.16 + (rz + 1) * .31})`;
      context.arc(px, py, (.6 + (rz + 1) * .5) * size / 650, 0, Math.PI * 2);
      context.fill();
      if (index % 5 === 0 && rz > 0) {
        context.beginPath();context.strokeStyle = 'rgba(227,174,255,.08)';
        context.moveTo(px, py);context.lineTo(px + 12, py + 8);context.stroke();
      }
    }
    if (!motion.matches && visible && !document.hidden) { frame++; animation = requestAnimationFrame(draw); }
  }
  function restart() { cancelAnimationFrame(animation); draw(); }
  new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; restart(); }).observe(canvas);
  motion.addEventListener('change', restart);
  document.addEventListener('visibilitychange', restart);
  window.addEventListener('resize', restart);
  draw();
}
