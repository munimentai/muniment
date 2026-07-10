<script>
  import { ringPath } from './mark.js'

  let { collapsed, subject, onToggle, onSignOut } = $props()
  let profileOpen = $state(false)
  const markD = ringPath()
</script>

<aside class:collapsed aria-label="Sidebar">
  <header>
    <button class="mark" onclick={onToggle} aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}>
      <svg width="24" height="24" viewBox="0 0 48 48" aria-hidden="true"><path d={markD} stroke-width="4.5" /></svg>
    </button>
    <span class="label wordmark">muniment</span>
    <button class="collapse label" onclick={onToggle} aria-label="Collapse sidebar">‹</button>
  </header>

  <nav aria-label="Thread navigation">
    <button disabled><span class="icon">＋</span><span class="label">New thread</span></button>
    <button disabled><span class="icon">⌕</span><span class="label">Search</span></button>
  </nav>

  <section class="threads" aria-labelledby="threads-label">
    <h2 id="threads-label" class="label">Threads</h2>
    <p class="empty label">No threads yet</p>
  </section>

  <div class="profile">
    {#if profileOpen}
      <div class="popover">
        <p>{subject}</p>
        <button onclick={onSignOut}>Sign out</button>
      </div>
    {/if}
    <button class="profile-button" onclick={() => profileOpen = !profileOpen} aria-expanded={profileOpen}>
      <span class="identity" aria-hidden="true">@</span>
      <span class="subject label">{subject}</span>
    </button>
  </div>
</aside>

<style>
  aside {
    width: 260px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface);
    border-right: 1px solid var(--border);
    transition: width 180ms ease;
  }
  aside.collapsed { width: 52px; }
  header { height: 54px; display: flex; align-items: center; gap: 9px; padding: 0 12px; }
  button { color: var(--ink); font: inherit; }
  .mark, .collapse, nav button, .profile-button, .popover button {
    border: 0;
    background: transparent;
  }
  .mark { display: flex; padding: 2px; cursor: pointer; }
  .mark path { fill: none; stroke: var(--ink); stroke-linecap: round; }
  .wordmark { font-weight: 600; letter-spacing: -0.01em; }
  .collapse { margin-left: auto; padding: 5px; color: var(--muted); cursor: pointer; }
  nav { padding: 8px 10px; border-top: 1px solid var(--border); }
  nav button { width: 100%; display: flex; align-items: center; gap: 10px; padding: 8px 10px; border-radius: var(--radius-control); text-align: left; color: var(--muted); }
  .icon { width: 18px; flex: none; text-align: center; font-family: var(--font-mono); }
  .threads { flex: 1; padding: 12px 20px; }
  h2 { font-size: 10.5px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted); }
  .empty { margin-top: 9px; color: var(--muted); font-family: var(--font-mono); font-size: var(--text-12); }
  .profile { position: relative; padding: 10px; border-top: 1px solid var(--border); }
  .profile-button { width: 100%; display: flex; align-items: center; gap: 10px; padding: 7px; border-radius: var(--radius-control); cursor: pointer; }
  .profile-button:hover { background: var(--faint); }
  .identity { width: 26px; height: 26px; flex: none; display: grid; place-items: center; color: var(--muted); background: var(--faint); border: 1px solid var(--border); border-radius: var(--radius-control); font-family: var(--font-mono); font-size: var(--text-12); }
  .subject { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono); font-size: var(--text-12); }
  .popover { position: absolute; right: 10px; bottom: 58px; left: 10px; padding: 8px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .popover p { padding: 6px 8px 10px; overflow-wrap: anywhere; color: var(--muted); font-family: var(--font-mono); font-size: var(--text-12); }
  .popover button { width: 100%; padding: 7px 8px; border-radius: var(--radius-control); text-align: left; cursor: pointer; }
  .popover button:hover { background: var(--faint); }
  .collapsed .label { display: none; }
  .collapsed header { justify-content: center; padding: 0; }
  .collapsed nav { padding: 8px 6px; }
  .collapsed nav button { justify-content: center; padding: 8px 0; }
  .collapsed .threads { padding: 0; }
  .collapsed .profile { padding: 10px 6px; }
  .collapsed .profile-button { justify-content: center; padding: 7px 0; }
  .collapsed .popover { width: 220px; right: auto; left: 42px; }
  @media (prefers-reduced-motion: reduce) { aside { transition: none; } }
</style>
