import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import ProjectCatalog from './ProjectCatalog.svelte'
afterEach(cleanup)

it('searches projects and opens only their assigned threads', async () => {
  const onthread = vi.fn(), onnewthread = vi.fn()
  render(ProjectCatalog, { projects: [['a', 'Research'], ['b', 'Writing']], threads: [{threadId:'one',title:'Sources'}, {threadId:'two',title:'Draft'}], assignments: {one:'a',two:'b'}, onthread, onnewthread })
  await fireEvent.input(screen.getByRole('searchbox'), {target:{value:'research'}})
  expect(screen.queryByRole('button', {name:/Writing/})).toBeNull()
  await fireEvent.click(screen.getByRole('button', {name:/Research/}))
  expect(screen.queryByRole('button', {name:'Draft'})).toBeNull()
  await fireEvent.click(screen.getByRole('button', {name:'Sources'}))
  expect(onthread).toHaveBeenCalledWith('one')
  await fireEvent.click(screen.getByRole('button', {name:'New thread'}))
  expect(onnewthread).toHaveBeenCalledWith('a')
  await fireEvent.click(screen.getByRole('button', {name:'Back to projects'}))
  expect(screen.getByRole('searchbox').value).toBe('research')
})

it('creates a named project and keeps the form open when creation fails', async () => {
  const oncreate = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true)
  render(ProjectCatalog, {oncreate})
  await fireEvent.click(screen.getByRole('button', {name:'New project'}))
  await fireEvent.input(screen.getByRole('textbox', {name:'Project name'}), {target:{value:' Research '}})
  await fireEvent.click(screen.getByRole('button', {name:'Create',exact:true}))
  await waitFor(() => expect(oncreate).toHaveBeenCalledWith('Research'))
  expect(screen.getByRole('textbox', {name:'Project name'})).not.toBeNull()
  await fireEvent.click(screen.getByRole('button', {name:'Create',exact:true}))
  await waitFor(() => expect(screen.queryByRole('textbox', {name:'Project name'})).toBeNull())
})
