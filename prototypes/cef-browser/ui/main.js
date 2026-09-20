const { invoke } = window.__TAURI__.core;
let view = 'browser';
const $ = id => document.getElementById(id);
const output = value => { $('output').textContent = typeof value === 'string' ? value : JSON.stringify(value, null, 2); };
async function command(action, value = '', selector = '') {
  return invoke('browser_command', { request: { view, action, value, selector } });
}
async function resize() { await invoke('select_view', { label: view, width: innerWidth, height: innerHeight }); }
async function refresh() {
  const state = await command('status');
  if (document.activeElement !== $('address')) $('address').value = state.url;
  $('permission').textContent = state.allowed ? 'Agent control allowed for this page' : 'You are in control';
  $('engine').textContent = `Chromium ${state.chromium} · Sandbox required`;
}
function run(fn) { return async event => { event?.preventDefault(); try { await fn(); } catch (e) { output(String(e)); } }; }
for (const name of ['browser', 'artifact']) $(name + '-tab').onclick = run(async () => {
  view = name;
  output('Ready.');
  $('capture-image').hidden = true;
  for (const n of ['browser', 'artifact']) $(n + '-tab').classList.toggle('selected', n === view);
  $('address').disabled = view === 'artifact';
  await resize(); await refresh();
});
$('navigation').onsubmit = run(async () => { let value = $('address').value.trim(); if (!value.includes('://')) value = 'https://' + value; await command('navigate', value); });
$('fixture').onclick = run(() => command('navigate', 'http://127.0.0.1:48763/' + (view === 'artifact' ? 'artifact' : '')));
$('reload').onclick = run(() => command('reload'));
$('grant').onclick = run(async () => { const r = await command('grant'); output(`Agent control allowed for ${r.origin}. Navigation ends access.`); await refresh(); });
$('stop').onclick = run(async () => { await command('stop'); output('Agent control stopped.'); await refresh(); });
$('read').onclick = run(async () => { const r = await command('snapshot'); const page = JSON.parse(r); output(page.title + '\n\n' + page.text); });
$('capture').onclick = run(async () => { const r = await command('screenshot'); $('capture-image').src = 'data:image/png;base64,' + r.data; $('capture-image').hidden = false; output('Captured the visible page.'); });
window.__TAURI__.event.listen('page-change', ({payload}) => { if (payload.view === view) { $('permission').textContent = 'You are in control'; } });
window.addEventListener('resize', run(resize));
run(async () => { await resize(); await refresh(); })();

window.__TAURI__.event.listen('agent-action', ({payload}) => { output(`Agent ${payload.action} · ${payload.view} · ${payload.ok ? "Complete" : "Blocked"}`); if (payload.action === "stop") run(refresh)(); });

// Read the parent browser URL after redirects and CEF-owned popups settle.
setInterval(() => refresh().catch(() => {}), 1500);
$('back').onclick = run(() => command('back'));
$('forward').onclick = run(() => command('forward'));
