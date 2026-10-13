export function invocations(items) {
  return items.filter(item => item.enabled !== false).flatMap(item => [
    ...(item.kind === 'plugin' ? [{ id: item.id, name: item.name, description: item.description, kind: 'plugin' }] : []),
    ...(item.skills || []).map(skill => ({ id: `${item.id}:${skill.path}`, name: skill.name, description: skill.description, kind: 'skill' })),
  ])
}
export function invocationQuery(text) {
  // Only a leading command token. Paths and code never open this picker.
  const match = text.match(/^[\\/]([\w-]*)$/)
  return match ? match[1].toLowerCase() : null
}

export function commandName(item) {
  return item.name.toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-|-$/g, '') || item.kind
}
// Commands the app runs itself. They come first, and an extension with the
// same name takes the next free one.
export const BUILT_IN_COMMANDS = [
  { id: 'builtin:compact', command: 'compact', name: 'compact', kind: 'built in', builtIn: true, description: 'Summarize older messages now. Text after it focuses the summary.' },
]
export function commandChoices(items) {
  const used = new Set(BUILT_IN_COMMANDS.map(item => item.command))
  return [...BUILT_IN_COMMANDS, ...invocations(items).map(item => {
    const base = commandName(item)
    let name = base, index = 2
    while (used.has(name)) name = `${base}-${index++}`
    used.add(name)
    return { ...item, command: name }
  })]
}
export function selectedCommands(prompt, choices) {
  const names = new Set([...prompt.matchAll(/(?:^|\s)\/([a-z0-9_-]+)(?=\s|$)/g)].map(match => match[1]))
  return choices.filter(item => !item.builtIn && names.has(item.command)).map(item => item.id)
}
