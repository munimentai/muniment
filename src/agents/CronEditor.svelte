<script>
  // A schedule: the friendly builder and the raw 5-field expression it writes,
  // with the time zone it fires in. The runtime checks the expression.
  import { INTERVALS, WEEKDAYS, builderFromCron, cronFromBuilder, intervalLabel } from './cron.js'
  let { schedule = $bindable(), label = 'Schedule', disabled = false } = $props()
  const id = $props.id()
  const zones = (() => { try { return Intl.supportedValuesOf('timeZone') } catch { return [] } })()
  const parsed = builderFromCron(schedule.expr)
  let mode = $state(parsed?.mode ?? 'custom')
  let time = $state(parsed?.time ?? '09:00')
  let weekday = $state(parsed?.weekday ?? '1')
  let minutes = $state(parsed?.minutes ?? 60)

  function write() {
    if (mode === 'custom') return
    schedule = { ...schedule, expr: cronFromBuilder({ mode, time, weekday, minutes: Number(minutes) }) }
  }
  // A typed expression moves the builder to the shape it reads as.
  function typed(event) {
    schedule = { ...schedule, expr: event.currentTarget.value }
    const choice = builderFromCron(schedule.expr)
    mode = choice?.mode ?? 'custom'
    if (choice?.time) time = choice.time
    if (choice?.weekday) weekday = choice.weekday
    if (choice?.minutes) minutes = choice.minutes
  }
</script>

<fieldset {disabled}>
  <legend>{label}</legend>
  <div class="fields">
    <label>Repeat<select bind:value={mode} onchange={write}>
      <option value="daily">Every day</option>
      <option value="weekdays">Weekdays</option>
      <option value="weekly">Every week</option>
      <option value="interval">At an interval</option>
      <option value="custom">Custom</option>
    </select></label>
    {#if mode === 'weekly'}<label>Day<select bind:value={weekday} onchange={write}>{#each WEEKDAYS as [value, name] (value)}<option {value}>{name}</option>{/each}</select></label>{/if}
    {#if ['daily', 'weekdays', 'weekly'].includes(mode)}<label>Time<input type="time" bind:value={time} oninput={write} required /></label>{/if}
    {#if mode === 'interval'}<label>Every<select bind:value={minutes} onchange={write}>{#each INTERVALS as value (value)}<option {value}>{intervalLabel(value).replace('Every ', '')}</option>{/each}</select></label>{/if}
  </div>
  <label>Expression<input class="mono" value={schedule.expr} oninput={typed} required spellcheck="false" autocomplete="off" aria-describedby={`${id}-hint`} /></label>
  <label>Time zone<input class="mono" list={`${id}-zones`} bind:value={schedule.tz} required spellcheck="false" autocomplete="off" /></label>
  <datalist id={`${id}-zones`}>{#each zones as zone (zone)}<option value={zone}></option>{/each}</datalist>
  <small id={`${id}-hint`}>Minute, hour, day, month and weekday. Runs at most every 5 minutes.</small>
</fieldset>

<style>
  fieldset { display: grid; gap: 8px; min-width: 0; margin: 0; padding: 0; border: 0; }
  legend { padding: 0; margin-bottom: 4px; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  .fields { display: flex; flex-wrap: wrap; gap: 8px; }
  .fields label { flex: 1 1 120px; }
  label { display: grid; gap: 4px; min-width: 0; font-size: var(--text-12); color: var(--muted); }
  input, select { box-sizing: border-box; width: 100%; min-width: 0; min-height: 30px; padding: 5px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); }
  .mono { font-family: var(--font-mono); font-size: var(--text-12); }
  small { color: var(--muted); font-size: var(--text-12); line-height: 1.5; }
</style>
