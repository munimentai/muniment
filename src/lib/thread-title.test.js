import { describe, expect, it } from 'vitest'

import { threadTitle, creationTitle, creationThreadTitle } from './thread-title.js'

describe('thread title', () => {
  it('uses a short first user prompt', () => {
    expect(threadTitle([{ role: 'user', text: 'Review the lease renewal' }])).toBe('Review the lease renewal')
  })

  it('limits a long prompt to 80 Unicode scalars including the ellipsis', () => {
    const title = threadTitle([{ role: 'user', text: '😀'.repeat(81) }])

    expect(Array.from(title)).toHaveLength(80)
    expect(title).toBe(`${'😀'.repeat(79)}…`)
  })

  it('collapses whitespace in a prompt', () => {
    expect(threadTitle([{ role: 'user', text: '  Review \n\t the   lease \r renewal  ' }])).toBe('Review the lease renewal')
  })

  it('uses the empty name for an empty transcript', () => {
    expect(threadTitle([])).toBe('New thread')
  })

  it.each([undefined, ' \n\t '])('uses the empty name when the first user prompt has no readable text', (text) => {
    expect(threadTitle([
      { role: 'assistant', text: 'Earlier response' },
      { role: 'user', text },
      { role: 'user', text: 'A later prompt' },
    ])).toBe('New thread')
  })
})

describe('creation titles', () => {
  it.each(['agent', 'artifact'])('uses the shared short thread name for a pending %s', kind => {
    const creation = {kind, threadId:'draft', goal:'Create a polished dashboard with charts and a checklist'}
    expect(creationTitle(creation, [{threadId:'draft', title:'Launch readiness'}])).toBe('Launch readiness')
    expect(creationTitle(creation, [{threadId:'draft', title:creation.goal}])).toBe(`New ${kind}`)
    // The user's own first words name the chat until the namer answers.
    expect(creationTitle(creation, [{threadId:'draft', title:'A countdown to New Year with big colorful digits.'}])).toBe('A countdown to New Year with big colorful digits.')
    // An older chat's scaffold never shows.
    expect(creationTitle(creation, [{threadId:'draft', title:`I want to create an ${kind} in this dedicated chat. Goal: Create a polished…`}])).toBe(`New ${kind}`)
  })
  it('keeps the record title when the thread list no longer holds the draft', () => {
    const creation = {kind:'agent', threadId:'draft', goal:'Report the Muniment project status every morning to the owner'}
    expect(creationThreadTitle(creation, [{threadId:'draft', title:'Muniment Project Status'}])).toBe('Muniment Project Status')
    expect(creationThreadTitle(creation, [{threadId:'other', title:'Other'}])).toBe('')
    expect(creationThreadTitle(creation, [{threadId:'draft', title:'New thread'}])).toBe('')
    expect(creationTitle({...creation, title:'Muniment Project Status'}, [{threadId:'other', title:'Other'}])).toBe('Muniment Project Status')
    expect(creationTitle({...creation, title:'Muniment Project Status'}, [])).toBe('Muniment Project Status')
    // The thread list's newer name still wins while it holds the chat.
    expect(creationTitle({...creation, title:'Muniment Project Status'}, [{threadId:'draft', title:'Morning status'}])).toBe('Morning status')
  })
  it('names an agent chat by its newest draft when nothing else names it', () => {
    const turns = [{message:'Watch the build', reply:{draft:{name:'Build watcher'}}}, {message:'Also email me', reply:{question:{text:'Which address?'}}}]
    expect(creationTitle({kind:'agent', threadId:'chat', goal:'Watch the build every hour and email me', turns}, [])).toBe('Build watcher')
  })
  it('keeps an explicit published name and a short renamed draft', () => {
    expect(creationTitle({kind:'artifact', goal:'Draft report'})).toBe('Draft report')
    expect(creationTitle({kind:'artifact'}, [], 'Launch readiness')).toBe('Launch readiness')
  })
})
