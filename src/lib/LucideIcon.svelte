<script>
  // Lucide icons, ISC, vendored as path data on the 24-unit grid.
  // `side` icons rest in muted ink on the rail and row, `action` icons take the control's color.
  let { name, size = 16, variant = 'side' } = $props()
</script>

<svg class="lucide" class:side-icon={variant === 'side'} class:action-icon={variant === 'action'} width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" data-icon={name}>
  {#if name === 'house'}
    <path d="m3 10 9-7 9 7v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1ZM9 21v-8h6v8" />
  {:else if name === 'file-plus'}
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8ZM14 2v6h6M12 12v6M9 15h6" />
  {:else if name === 'scale'}
    <path d="M12 3v18" /><path d="m19 8 3 8a5 5 0 0 1-6 0zV7" /><path d="M3 7h1a17 17 0 0 0 8-2 17 17 0 0 0 8 2h1" /><path d="m5 8 3 8a5 5 0 0 1-6 0zV7" /><path d="M7 21h10" />
  {:else if name === 'user'}
    <circle cx="12" cy="8" r="5" /><path d="M20 21a8 8 0 0 0-16 0" />
  {:else if name === 'cpu'}
    <rect x="4" y="4" width="16" height="16" rx="2" /><rect x="9" y="9" width="6" height="6" /><path d="M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3" />
  {:else if name === 'route'}
    <circle cx="6" cy="19" r="3" /><path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15" /><circle cx="18" cy="5" r="3" />
  {:else if name === 'gauge'}
    <path d="m12 14 4-4" /><path d="M3.34 19a10 10 0 1 1 17.32 0" />
  {:else if name === 'pencil-sparkles'}
    <path d="M10 3H8" /><path d="m15.007 5.008 3.987 3.986" /><path d="M20 15v4" /><path d="M21.174 6.813a2.82 2.82 0 0 0-3.986-3.987L3.842 16.175a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" /><path d="M22 17h-4" /><path d="M4 5v4" /><path d="M6 7H2" /><path d="M9 2v2" />
  {:else if name === 'unplug'}
    <path d="m19 5 3-3" /><path d="m2 22 3-3" /><path d="M6.3 20.3a2.4 2.4 0 0 0 3.4 0L12 18l-6-6-2.3 2.3a2.4 2.4 0 0 0 0 3.4Z" /><path d="M7.5 13.5 10 11" /><path d="M10.5 16.5 13 14" /><path d="m12 6 6 6 2.3-2.3a2.4 2.4 0 0 0 0-3.4l-2.6-2.6a2.4 2.4 0 0 0-3.4 0Z" />
  {:else if name === 'sliders-vertical'}
    <path d="M10 8h4" /><path d="M12 21v-9" /><path d="M12 8V3" /><path d="M17 16h4" /><path d="M19 12V3" /><path d="M19 21v-5" /><path d="M3 14h4" /><path d="M5 10V3" /><path d="M5 21v-7" />
  {:else if name === 'sliders-horizontal'}
    <path d="M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4" />
  {:else if name === 'layout-dashboard'}
    <rect x="3" y="3" width="7" height="9" rx="1" /><rect x="14" y="3" width="7" height="5" rx="1" /><rect x="14" y="12" width="7" height="9" rx="1" /><rect x="3" y="16" width="7" height="5" rx="1" />
  {:else if name === 'folder-plus'}
    <path d="M20 20H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2ZM12 10v6M9 13h6" />
  {:else if name === 'ellipsis-vertical'}
    <circle cx="12" cy="5" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="12" cy="19" r="1" />
  {:else if name === 'menu'}
    <path d="M4 6h16M4 12h16M4 18h16" />
  {:else if name === 'image'}
    <rect x="3" y="3" width="18" height="18" rx="2" /><circle cx="8.5" cy="8.5" r="1.5" /><path d="m21 15-5-5L5 21" />
  {:else if name === 'file-code'}
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Zm0 0v6h6M10 13l-2 2 2 2m4-4 2 2-2 2" />
  {:else if name === 'sheet'}
    <rect x="3" y="3" width="18" height="18" rx="2" /><path d="M3 9h18M3 15h18M9 9v12M15 9v12" />
  {:else if name === 'music'}
    <path d="M9 18V5l12-2v13" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="16" r="3" />
  {:else if name === 'video'}
    <rect x="3" y="5" width="13" height="14" rx="2" /><path d="m16 10 5-3v10l-5-3" />
  {:else if name === 'settings'}
    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === 'panel-left-open'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <path d="M9 3v18" />
    <path d="m14 9 3 3-3 3" />
  {:else if name === 'panel-left-close'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <path d="M9 3v18" />
    <path d="m16 15-3-3 3-3" />
  {:else if name === 'panel-right-open'}
    <rect width="18" height="18" x="3" y="3" rx="2" /><path d="M15 3v18m-5-6-3-3 3-3" />
  {:else if name === 'panel-right-close'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
    <path d="M15 3v18m-7-6 3-3-3-3" />
  {:else if name === 'bot'}
    <rect x="4" y="7" width="16" height="14" rx="3" /><path d="M12 7V3H9M1 12v5M23 12v5M8 16h.01M16 16h.01" />
  {:else if name === 'file'}
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Zm0 0v6h6" />
  {:else if name === 'arrow-right'}
    <path d="M5 12h14m-7-7 7 7-7 7" />
  {:else if name === 'folder'}
    <path d="M20 20H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2Z" />
  {:else if name === 'folder-open'}
    <path d="m6 14 2-4h14l-3 10H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h5l2 2h9v4" />
  {:else if name === 'square-pen'}
    <path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
    <path d="M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z" />
  {:else if name === 'refresh-cw'}
    <path d="M3 12a9 9 0 0 1 15.36-6.36L21 8M21 3v5h-5M21 12a9 9 0 0 1-15.36 6.36L3 16M8 16H3v5" />
  {:else if name === 'pencil'}
    <path d="m16 3 5 5M3 21l1-6L16 3a2 2 0 0 1 5 5L9 20Z" />
  {:else if name === 'paperclip'}
    <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l10.6-10.6a4 4 0 0 1 5.66 5.66L9.41 17.41a2 2 0 0 1-2.83-2.83l9.2-9.19" />
  {:else if name === 'ellipsis'}
    <circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" />
  {:else if name === 'archive'}
    <rect x="3" y="3" width="18" height="4" rx="1" /><path d="M5 7v13h14V7M10 11h4" />
  {:else if name === 'plus'}
    <path d="M5 12h14" />
    <path d="M12 5v14" />
  {:else if name === 'play'}
    <path d="M6 3l14 9-14 9V3z" />
  {:else if name === 'book-open'}
    <path d="M12 5v16" />
    <path d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z" />
  {:else if name === 'square-terminal'}
    <path d="m7 11 2-2-2-2" /><path d="M11 13h4" />
    <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
  {:else if name === 'file-text'}
    <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
    <path d="M14 2v4a2 2 0 0 0 2 2h4" />
    <path d="M10 9H8" />
    <path d="M16 13H8" />
    <path d="M16 17H8" />
  {:else if name === 'chevron-left'}
    <path d="m15 18-6-6 6-6" />
  {:else if name === 'chevron-up'}
    <path d="m18 15-6-6-6 6" />
  {:else if name === 'chevron-right'}
    <path d="m9 18 6-6-6-6" />
  {:else if name === 'maximize-2'}
    <path d="M15 3h6v6" />
    <path d="M9 21H3v-6" />
    <path d="M21 3l-7 7" />
    <path d="M3 21l7-7" />
  {:else if name === 'minimize-2'}
    <path d="M4 14h6v6" />
    <path d="M20 10h-6V4" />
    <path d="M14 10l7-7" />
    <path d="M3 21l7-7" />
  {:else if name === 'mic'}
    <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
    <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
    <path d="M12 19v3" />
  {:else if name === 'trash-2'}
    <path d="M3 6h18" />
    <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" />
    <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
    <path d="M10 11v6" />
    <path d="M14 11v6" />
  {:else if name === 'log-in'}
    <path d="m10 17 5-5-5-5" />
    <path d="M15 12H3" />
    <path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4" />
  {:else if name === 'copy'}
    <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
    <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
  {:else if name === 'check'}
    <path d="M20 6 9 17l-5-5" />
  {:else if name === 'x'}
    <path d="M18 6 6 18" />
    <path d="m6 6 12 12" />
  {:else if name === 'arrow-down'}
    <path d="M12 5v14m-7-7 7 7 7-7" />
  {:else if name === 'arrow-up'}
    <path d="m5 12 7-7 7 7" />
    <path d="M12 19V5" />
  {:else if name === 'square'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
  {:else if name === 'clock'}
    <circle cx="12" cy="12" r="10" />
    <path d="M12 6v6l4 2" />
  {:else if name === 'arrow-left'}
    <path d="m12 19-7-7 7-7" />
    <path d="M19 12H5" />
  {:else if name === 'search'}
    <circle cx="11" cy="11" r="8" />
    <path d="m21 21-4.3-4.3" />
  {:else if name === 'refresh-ccw-dot'}
    <path d="M3 2v6h6" />
    <path d="M21 12A9 9 0 0 0 6 5.3L3 8" />
    <path d="M21 22v-6h-6" />
    <path d="M3 12a9 9 0 0 0 15 6.7l3-2.7" />
    <circle cx="12" cy="12" r="1" />
  {:else if name === 'list-collapse'}
    <path d="M10 5h11M10 12h11M10 19h11m-18-9 3-3-3-3m0 16 3-3-3-3" />
  {:else if name === 'globe'}
    <circle cx="12" cy="12" r="10" /><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20M2 12h20" />
  {:else if name === 'chevron-down'}
    <path d="m6 9 6 6 6-6" />
  {/if}
</svg>

<style>
  /* §1.7: one geometric 1.6px-stroke icon set. */
  .lucide { flex: none; display: block; }
  .lucide :is(rect, path, circle) { fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .side-icon { color: var(--muted); }
  .action-icon { color: inherit; }
</style>
