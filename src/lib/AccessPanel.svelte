<script>
  // The signed-in person at the foot of the sidebar. Their name opens one menu
  // item, Sign out. Everything else about the account lives in Settings.
  import { onMount } from 'svelte'

  let { tauri, subject, onSignOut } = $props()
  let profileSnapshot = $state(null)
  let menuOpen = $state(false)
  let profileButton = $state()
  let profileMenu = $state()
  let profileName = $derived(profileSnapshot?.user_display_name ?? subject ?? 'Signed in')
  let profileDetails = $derived(profileSnapshot ? `${profileSnapshot.organization_display_name ?? profileSnapshot.org_id} · ${profileSnapshot.role}` : 'Access unavailable')

  async function loadProfile() {
    try {
      profileSnapshot = await tauri.invoke('auth_entitlement_snapshot')
    } catch (_) {
      profileSnapshot = null
    }
  }

  function openMenu() {
    menuOpen = true
    requestAnimationFrame(() => profileMenu?.querySelector('button')?.focus())
  }

  function closeMenu() {
    if (!menuOpen) return
    menuOpen = false
    profileButton?.focus()
  }

  onMount(() => {
    loadProfile()
    const outside = (event) => {
      const path = event.composedPath()
      if (menuOpen && !path.includes(profileMenu) && !path.includes(profileButton)) closeMenu()
    }
    const escape = (event) => {
      if (!menuOpen || event.key !== 'Escape') return
      event.preventDefault()
      closeMenu()
    }
    document.addEventListener('click', outside)
    document.addEventListener('keydown', escape)
    return () => {
      document.removeEventListener('click', outside)
      document.removeEventListener('keydown', escape)
    }
  })
</script>

<div class="profile-block">
  <button bind:this={profileButton} class="profile-button" aria-haspopup="menu" aria-expanded={menuOpen} onclick={() => menuOpen ? closeMenu() : openMenu()}><span><strong>{profileName}</strong><small>{profileDetails}</small></span></button>
  {#if menuOpen}
    <div bind:this={profileMenu} class="profile-menu" role="menu" aria-label="Account">
      <button type="button" role="menuitem" onclick={() => { closeMenu(); onSignOut() }}>Sign out</button>
    </div>
  {/if}
</div>

<style>
  .profile-block { position: relative; margin-top: auto; padding-top: 10px; border-top: 1px solid var(--border); }
  .profile-button { width: 100%; display: flex; align-items: center; gap: 9px; padding: 9px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; font-size: var(--text-13); text-align: left; cursor: pointer; }
  .profile-button:hover { background: var(--faint); }
  .profile-button > span:last-child { min-width: 0; display: grid; }
  .profile-button strong { overflow: hidden; text-overflow: ellipsis; font-size: var(--text-13); }
  .profile-button small { overflow: hidden; color: var(--muted); font: var(--text-12) var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
  /* The menu carries the shell's menu padding: 4px around items of 3px 8px. */
  .profile-menu { position: absolute; z-index: 5; left: 0; right: 0; bottom: calc(100% + 6px); padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); }
  .profile-menu button { width: 100%; min-width: 24px; min-height: 24px; padding: 3px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; font-size: var(--text-13); text-align: left; cursor: pointer; }
  .profile-menu button:hover { background: var(--faint); }
</style>
