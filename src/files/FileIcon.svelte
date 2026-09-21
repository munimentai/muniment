<script>
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { fileIcon } from './file-icon.js'
  let { name, directory = false } = $props()
  const icon = $derived(fileIcon(name))
</script>
{#if directory}<LucideIcon name="folder" />
{:else}<span class="file-icon" aria-hidden="true" style:--file-light={icon.light} style:--file-dark={icon.dark}>{icon.character}</span>{/if}
<style>
  @font-face { font-family:Seti; src:url('./seti/seti.woff') format('woff'); font-display:block; }
  .file-icon { display:inline-flex; width:16px; height:16px; flex:none; align-items:center; justify-content:center; font:var(--text-22)/1 Seti; color:light-dark(var(--file-light),var(--file-dark)); }
</style>
