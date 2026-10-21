<script>
  // Settings → General → Time: the time zone and the clock every time in the
  // app reads in. Core keeps the choice on this computer as its `time`
  // preference, and each choice applies at once.
  import { onMount } from 'svelte'
  import PageHead from './ui/PageHead.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import SegmentedField from './ui/SegmentedField.svelte'
  import { CLOCK_OPTIONS, TIME_EVENT, saveTimeSetting, systemClock, systemTimeZone, timeSetting, timeZones } from './time-format.svelte.js'

  let { tauri } = $props()
  let setting = $state(timeSetting())
  let choosing = $state(!!timeSetting().zone)
  let query = $state('')
  let error = $state('')
  const zones = timeZones()
  const MAX_MATCHES = 40
  // A zone matches when it holds every word, with `_` and `/` read as spaces.
  const matches = $derived.by(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean)
    return zones.filter((zone) => words.every((word) => zone.toLowerCase().replace(/[_/]/g, ' ').includes(word) || zone.toLowerCase().includes(word))).slice(0, MAX_MATCHES)
  })
  const clockWords = { '12h': '12-hour', '24h': '24-hour' }

  onMount(() => {
    const follow = (event) => { setting = event.detail; if (setting.zone) choosing = true }
    window.addEventListener(TIME_EVENT, follow)
    return () => window.removeEventListener(TIME_EVENT, follow)
  })

  async function save(change) {
    try {
      setting = await saveTimeSetting(tauri, change)
      error = ''
    } catch (reason) {
      setting = timeSetting()
      error = String(reason?.message ?? reason)
    }
  }

  function pickSource(value) {
    choosing = value === 'zone'
    if (!choosing && setting.zone) void save({ zone: '' })
  }
</script>

<div class="time-preferences">
  <PageHead intro="The time zone and clock for every time in the app, on this computer." />
  <Group heading="Time zone and clock">
    <Row title="Time zone" description={setting.zone || `This computer: ${systemTimeZone()}`}>
      <SegmentedField label="Time zone" hideLabel value={choosing ? 'zone' : 'computer'} options={[{ value: 'computer', label: 'Match this computer' }, { value: 'zone', label: 'Choose a zone' }]} onchange={pickSource} />
    </Row>
    {#if choosing}
      <Row title="Zone" stacked>
        <div class="zone-picker">
          <input type="search" aria-label="Search time zones" placeholder="Search time zones" spellcheck="false" autocomplete="off" bind:value={query} />
          <div class="zones" role="listbox" aria-label="Time zones">{#each matches as zone (zone)}<button type="button" role="option" aria-selected={zone === setting.zone} onclick={() => save({ zone })}>{zone}</button>{:else}<p class="empty">No time zone matches.</p>{/each}</div>
        </div>
      </Row>
    {/if}
    <Row title="Clock" description={setting.clock ? '' : `This computer uses ${clockWords[systemClock()]} time.`}>
      <SegmentedField label="Clock" hideLabel value={setting.clock} options={CLOCK_OPTIONS.map(([value, label]) => ({ value, label }))} onchange={(clock) => save({ clock })} />
    </Row>
  </Group>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .time-preferences { display: grid; gap: var(--page-gap); }
  .zone-picker { display: grid; gap: 6px; width: 100%; min-width: 0; }
  input { box-sizing: border-box; width: 100%; min-height: 32px; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  .zones { display: grid; gap: 2px; max-height: 200px; overflow: auto; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); }
  .zones button { padding: 6px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-mono); text-align: left; cursor: pointer; }
  .zones button:hover, .zones button:focus-visible, .zones button[aria-selected="true"] { background: var(--faint); }
  .empty { margin: 0; padding: 6px 8px; color: var(--muted); font-size: var(--text-13); }
  .error { margin: 0; color: var(--danger); font-size: var(--text-13); }
</style>
