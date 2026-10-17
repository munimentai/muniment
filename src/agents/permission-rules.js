// The permission editor's view of an agent's rules. Each rule is a thread
// permission policy request, `{ request_kind, resource }`, and the runtime
// matches it exactly.

// The rule lists of an agent and of the runtime's global rules, in the order
// the gate reads them: a deny rule wins over ask first, and ask first over
// always allow.
export const RULE_LISTS = [['deny', 'Deny'], ['ask_first', 'Ask first'], ['always_allow', 'Always allow']]

export const RULE_KINDS = [['command', 'Command'], ['path', 'File'], ['tool', 'Tool']]
export const PATH_OPERATIONS = ['read', 'write', 'edit', 'ls', 'find', 'grep']
export const SHELLS = ['bash', 'powershell', 'bg_run']

export function ruleLabel(request) {
  const resource = request?.resource ?? {}
  if (resource.type === 'command') return `${(resource.arguments ?? []).join(' ')} in ${resource.working_directory}`
  if (resource.type === 'path') return `${resource.operation} ${resource.path}`
  if (resource.type === 'tool') {
    const input = JSON.stringify(resource.input ?? {})
    return input === '{}' ? resource.name : `${resource.name} ${input}`
  }
  return JSON.stringify(request)
}

export function emptyRuleForm(kind = 'command') {
  return { kind, shell: 'bash', command: '', directory: '', path: '', operation: 'read', name: '', input: '{}' }
}

// The policy request a filled form names. Throws an error the form shows.
export function ruleFromForm(form) {
  if (form.kind === 'command') {
    if (!form.command.trim() || !form.directory.trim()) throw new Error('Enter the command and the folder it runs in.')
    return { request_kind: 'command', resource: { type: 'command', arguments: [form.shell, form.command.trim()], working_directory: form.directory.trim() } }
  }
  if (form.kind === 'path') {
    if (!form.path.trim()) throw new Error('Enter the full path of the file.')
    return { request_kind: 'path', resource: { type: 'path', path: form.path.trim(), operation: form.operation } }
  }
  if (!form.name.trim()) throw new Error('Enter the tool name.')
  let input
  try { input = JSON.parse(form.input.trim() || '{}') } catch { throw new Error('Enter the tool input as JSON.') }
  return { request_kind: 'tool', resource: { type: 'tool', name: form.name.trim(), input } }
}

export function sameRule(a, b) {
  return JSON.stringify(a) === JSON.stringify(b)
}
