import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import UserQuestion from './UserQuestion.svelte'
import { readFileSync } from 'node:fs'
import vm from 'node:vm'
afterEach(cleanup)
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.open = true }
  HTMLDialogElement.prototype.close = function () { this.open = false }
})
const questions = [{id:'kind',question:'What should it show?',options:[{label:'Dashboard'},{label:'Calculator'}]}, {id:'data',question:'Which data?',multiSelect:true,options:[{label:'Sales'},{label:'Costs'}]}]
it('preserves answers when closed, requires explicit answers, and submits structured choices and text', async () => {
  const onanswer = vi.fn()
  render(UserQuestion, {payload:JSON.stringify({questions}),onanswer})
  expect(screen.getByRole('radio',{name:'Dashboard'})).not.toBeChecked()
  await fireEvent.click(screen.getByRole('radio',{name:'Dashboard'}))
  await fireEvent.click(screen.getByRole('button',{name:'Close'}))
  expect(onanswer).not.toHaveBeenCalled()
  await fireEvent.click(screen.getByRole('button',{name:'Answer questions'}))
  expect(screen.getByRole('radio',{name:'Dashboard'})).toBeChecked()
  await fireEvent.click(screen.getByRole('button',{name:'Next'}))
  expect(screen.getByRole('button',{name:'Submit answers'})).toBeDisabled()
  await fireEvent.click(screen.getByRole('checkbox',{name:'Sales'}))
  await fireEvent.click(screen.getByRole('checkbox',{name:'Costs'}))
  await fireEvent.input(screen.getByRole('textbox'),{target:{value:'Use this quarter'}})
  await fireEvent.click(screen.getByRole('button',{name:'Submit answers'}))
  expect(JSON.parse(onanswer.mock.calls[0][0]).answers).toEqual([
    {id:'kind',question:questions[0].question,selected:['Dashboard'],text:''},
    {id:'data',question:questions[1].question,selected:['Sales','Costs'],text:'Use this quarter'}
  ])
})
it('registers a waiting Pi tool and returns answers without a timeout or inferred defaults', async () => {
  let tool
  vm.runInNewContext(readFileSync('src-tauri/core/src/extensions/ask-user-question.js','utf8'), {pi:{registerTool:value=>{tool=value}}})
  expect(tool.name).toBe('ask_user_question')
  let resolve
  const editor = vi.fn(() => new Promise(done=>{resolve=done}))
  const result = tool.execute('id',{questions:[questions[0]]},null,null,{ui:{editor}})
  expect(editor).toHaveBeenCalledWith('muniment:ask_user_question',JSON.stringify({questions:[questions[0]]}))
  const answer={answers:[{id:'kind',selected:['Calculator'],text:''}]}
  resolve(JSON.stringify(answer))
  expect((await result).details).toEqual(answer)
  editor.mockResolvedValue(undefined)
  expect((await tool.execute('id',{questions},null,null,{ui:{editor}})).details.cancelled).toBe(true)
  editor.mockResolvedValue(JSON.stringify({answers:[{id:'kind',selected:['Invented'],text:''}]}))
  await expect(tool.execute('id',{questions:[questions[0]]},null,null,{ui:{editor}})).rejects.toThrow('does not match')
})

it('stops a question tool while its editor still awaits a response', async () => {
  let tool
  vm.runInNewContext(readFileSync('src-tauri/core/src/extensions/ask-user-question.js', 'utf8'), {pi: {registerTool: value => { tool = value }}})
  const controller = new AbortController()
  const editor = vi.fn(() => new Promise(() => {}))
  const result = tool.execute('id', {questions}, controller.signal, null, {ui: {editor}})
  controller.abort()
  expect((await result).details.cancelled).toBe(true)
})
