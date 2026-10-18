import '@testing-library/jest-dom/vitest'
import fs from 'node:fs'
import path from 'node:path'
import { cleanup, render, fireEvent } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import ActionFeedback from './ActionFeedback.svelte'

afterEach(cleanup)

const gallery = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/tool-presentation/presented.json'), 'utf8'))
const call = (id) => gallery.calls.find((activity) => activity.effectId === id)
const command = (effectId, command, extra = {}) => ({ effectId, displayName: 'bash', status: 'completed', input: JSON.stringify({ command }), presentation: { kind: 'command', icon: 'square-terminal', headline: `Run \`${command}\``, progress: `Running ${command}`, result: 'Exit 0 in 0.2 s' }, ...extra })

it('opens the group and action details and stops the active sheen when the run ends', async () => {
  const activities = [command('command-0', 'pwd'), command('command-1', 'cargo test', { status: 'running', output: '12 tests passed' })]
  const view = render(ActionFeedback, { activities, live: true })
  const tools = view.getByText('2 tools used').closest('details')
  expect(tools).not.toHaveAttribute('open')
  await fireEvent.click(tools.querySelector('summary'))
  expect(tools).toHaveAttribute('open')
  const group = view.getByText('Running commands').closest('details')
  expect(group).not.toHaveAttribute('open')
  await fireEvent.click(group.querySelector('summary'))
  expect(group).toHaveAttribute('open')
  const action = view.getByText('Running cargo test').closest('details')
  await fireEvent.click(action.querySelector('summary'))
  expect(action).toHaveAttribute('open')
  await fireEvent.click(view.getAllByText('Show details').at(-1))
  expect(view.getByText('12 tests passed')).toBeVisible()
  expect(view.container.querySelectorAll('.active-sheen')).toHaveLength(3)
  await view.rerender({ activities, live: false })
  expect(view.container.querySelector('.active-sheen')).toBeNull()
  expect(view.getAllByText('Interrupted')).toHaveLength(2)
})

it('opens read files in the panel and names the result beside the headline', async () => {
  const opened = []
  const view = render(ActionFeedback, { onopenfile: (file) => opened.push(file), activities: [call('read')] })
  const read = view.getByRole('button', { name: 'Read README.md', hidden: true })
  await fireEvent.click(read)
  expect(opened).toEqual([{ path: '/Users/sam/work/muniment/README.md' }])
})

it('shows a web search as its headline and details, with the raw input behind Show details', async () => {
  const view = render(ActionFeedback, { activities: [call('web')] })
  expect(view.getByText('1 tool used')).toBeInTheDocument()
  const row = view.container.querySelector('.action-list details')
  expect(row.querySelector('summary')).toHaveTextContent('Search the web for “Local desktop October 2026” and 1 more on news.ycombinator.com Found 8 results')
  await fireEvent.click(row.querySelector('summary'))
  const details = row.querySelector('.tool-details')
  expect(details).toHaveTextContent('Results per query8')
  expect(details).toHaveTextContent('Page contentIncluded')
  expect(details).not.toHaveTextContent('Provider')
  const raw = row.querySelector('.tool-raw')
  expect(raw).not.toHaveAttribute('open')
  expect(raw).toHaveTextContent('"numResults": 8')
})

it('never shows raw JSON outside Show details for any call in the gallery', async () => {
  const view = render(ActionFeedback, { activities: gallery.calls, live: true })
  for (const summary of view.container.querySelectorAll('summary')) await fireEvent.click(summary)
  const shown = view.container.cloneNode(true)
  for (const raw of shown.querySelectorAll('.tool-raw')) raw.remove()
  expect(shown.textContent).not.toMatch(/[{[]"|":/)
  expect(view.container.querySelectorAll('.tool-raw').length).toBeGreaterThan(10)
})

it('reads the whole output of a truncated action when its detail opens', async () => {
  const reads = []
  let answer
  const loadoutput = (effectId) => {
    reads.push(effectId)
    return new Promise((resolve, reject) => { answer = { resolve, reject } })
  }
  const activities = [
    command('log', 'cat build.log', { output: 'first lines', outputBytes: 400_000, outputTruncated: true }),
    command('short', 'pwd', { output: '/work', outputBytes: 5 }),
  ]
  const view = render(ActionFeedback, { activities, loadoutput })
  await fireEvent.click(view.getByText('2 tools used').closest('summary'))
  await fireEvent.click(view.getByText('Ran commands').closest('summary'))
  await fireEvent.click(view.getByText('pwd').closest('summary'))
  const action = view.getByText('cat build.log').closest('details')
  await fireEvent.click(action.querySelector('summary'))
  await vi.waitFor(() => expect(reads).toEqual(['log']))
  expect(view.getByText('first lines')).toBeInTheDocument()
  expect(view.getByText('Loading the full output.')).toBeInTheDocument()

  answer.reject(new Error('unavailable'))
  const retry = await view.findByRole('button', { name: 'Load full output' })
  expect(view.getByRole('alert')).toHaveTextContent('The full output could not be loaded.')
  await fireEvent.click(retry)
  expect(reads).toEqual(['log', 'log'])
  answer.resolve({ kind: 'tool_output', text: 'all of the build log', bytes: 400_000, truncated: true })
  expect(await view.findByText('all of the build log')).toBeInTheDocument()
  expect(view.getByText('Showing the first 20 B of 391 KB.')).toBeInTheDocument()
})

it('nests a script call under its script and keeps the script one disclosure away', async () => {
  const script = (effectId, uses) => ({ effectId, displayName: 'codemode', status: 'completed', input: JSON.stringify({ code: `await tools.${uses}({})` }), presentation: { category: 'code', kind: 'command', icon: 'file-code', headline: `Run a script that uses ${uses}`, progress: 'Running a script', result: 'Script completed' } })
  const made = (effectId, parentEffectId, headline) => ({ effectId, parentEffectId, displayName: 'web_search', status: 'completed', input: '{"query":"x"}', presentation: { category: 'web', kind: 'web', icon: 'globe', headline, progress: 'Searching the web', result: 'Found 2 results', via: 'via script' } })
  const view = render(ActionFeedback, { activities: [script('s1', 'Web search'), made('s1/1', 's1', 'Search A'), script('s2', 'Fetch content'), made('s2/1', 's2', 'Fetch 3 pages'), made('s2/2', 's2', 'Fetch 1 page')] })
  await fireEvent.click(view.getByText('3 tools used').closest('summary'))
  // One call stands in for its script, with the note.
  const single = view.getByText('Search A').closest('details')
  expect(single.querySelector('summary')).toHaveTextContent('via script')
  expect(single.querySelector('summary')).not.toHaveTextContent('Run a script')
  await fireEvent.click(single.querySelector('summary'))
  expect(single).toHaveAttribute('open')
  expect(single.querySelector('.action-detail')).toHaveTextContent('Run a script that uses Web search')
  expect(single.querySelectorAll('details.tool-raw')).toHaveLength(2)
  // Several calls nest under their script's row.
  const nested = view.getByText('Fetch 3 pages').closest('.action-list.nested')
  expect(nested).toHaveTextContent('Fetch 1 page')
  expect(nested.previousElementSibling).toHaveTextContent('Run a script that uses Fetch content')
  expect(nested).not.toHaveTextContent('via script')
})
