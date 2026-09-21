// @vitest-environment node
import { afterEach, describe, expect, it } from 'vitest'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
const roots=[]
afterEach(()=>{for(const root of roots.splice(0))fs.rmSync(root,{recursive:true,force:true})})
function fixture(){
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'muniment-extend-'));roots.push(root)
 const source=path.join(root,'source');fs.mkdirSync(source);fs.writeFileSync(path.join(source,'SKILL.md'),'---\nname: review\ndescription: Review code\n---\nReview the user request.\n')
 const call=(action,data={})=>{
  const result=spawnSync(process.execPath,[path.resolve('src-tauri/core/src/extend_bridge.mjs')],{env:{...process.env,MUNIMENT_EXTEND_ROOT:root},input:JSON.stringify({action,data}),encoding:'utf8',timeout:10000})
  if(result.error)throw result.error
  return JSON.parse(result.stdout.split('MUNIMENT_EXTEND_RESULT=')[1])
 }
 return {root,source,call}
}
describe('local extension installation',()=>{
 it('pins a copy, detects review changes, and restores the previous version',()=>{
  const {source,call}=fixture()
  const first=call('preview',{source,kind:'skill'}).result
  expect(first.name).toBe('review')
  const installed=call('install',{previewId:first.id,skills:['SKILL.md']}).result.items[0]
  fs.writeFileSync(path.join(source,'SKILL.md'),'---\nname: next\ndescription: Next skill\n---\nNext version.\n')
  expect(fs.readFileSync(path.join(installed.base,'SKILL.md'),'utf8')).toContain('name: review')
  const next=call('preview',{source,kind:'skill'}).result
  const updated=call('install',{previewId:next.id,replaceId:installed.id,skills:['SKILL.md']})
  expect(updated.result.items[0].name).toBe('next')
  expect(call('rollback',{id:installed.id}).result.items[0].name).toBe('review')
  const changed=call('preview',{source,kind:'skill'}).result
  fs.appendFileSync(path.join(changed.base,'SKILL.md'),'Changed after review.')
  expect(call('install',{previewId:changed.id,skills:['SKILL.md']}).error).toMatch(/changed after review/)
 })
 it('rejects symlinks, unsupported hooks and plaintext server secrets',()=>{
  const {source,root,call}=fixture()
  fs.symlinkSync(path.join(source,'SKILL.md'),path.join(source,'link'))
  expect(call('preview',{source,kind:'skill'}).ok).toBe(false)
  fs.unlinkSync(path.join(source,'link'));fs.mkdirSync(path.join(source,'hooks'));fs.writeFileSync(path.join(source,'hooks/hooks.json'),'{}')
  expect(call('preview',{source,kind:'plugin'}).error).toMatch(/hooks/)
  expect(call('server',{name:'Private',definition:{url:'https://example.com/mcp',headers:{Authorization:'Bearer private'}}}).ok).toBe(false)
  expect(fs.existsSync(path.join(root,'extensions/inventory.json'))).toBe(false)
 })
 it('keeps toggles separate between chats and resets automatic selections',()=>{
  const {call}=fixture()
  call('turn',{threadId:'one',disabled:['docs'],selected:['review'],automatic:true})
  call('turn',{threadId:'two',disabled:[],selected:[]})
  const state=call('read').result
  expect(state.turns.one.disabled).toEqual(['docs'])
  expect(state.turns.two.disabled).toEqual([])
  expect(state.turns.one.automaticSelected).toEqual([])
 })
})

it('rejects catalog relay endpoints before connecting', () => {
  const {call} = fixture()
  for (const url of ['https://microsoft365.mcp.claude.com/mcp','https://hcls.mcp.claude.com./mcp','https://example-server.modelcontextprotocol.io/pdf/mcp']) {
    expect(call('server',{name:'Example',definition:{url}}).error).toMatch(/direct MCP server URL/)
  }
  expect(call('server',{name:'Notion',definition:{url:'https://mcp.notion.com/mcp'}}).ok).toBe(true)
})
