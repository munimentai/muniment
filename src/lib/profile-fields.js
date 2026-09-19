const headings = { name: 'Name', preferredName: 'Preferred name', work: 'Work', instructions: 'Instructions' }
export function parseProfile(text = '') {
  const fields = { name: '', preferredName: '', work: '', instructions: '' }
  let current = null
  const extra = []
  for (const line of text.replace(/\r\n/g, '\n').split('\n')) {
    if (/^# Profile\s*$/i.test(line)) continue
    const heading = line.match(/^##\s+(.+?)\s*$/)
    if (heading) {
      current = Object.keys(headings).find(key => headings[key].toLowerCase() === heading[1].toLowerCase()) || null
      if (!current) extra.push(line)
    } else if (current) fields[current] += `${line}\n`
    else extra.push(line)
  }
  for (const key of Object.keys(fields)) fields[key] = fields[key].trim()
  return { ...fields, extra: extra.join('\n').trim() }
}
export function serializeProfile(fields) {
  return '# Profile\n\n' + Object.entries(headings).map(([key, title]) => `## ${title}\n\n${fields[key].trim()}\n`).join('\n') + (fields.extra?.trim() ? `\n${fields.extra.trim()}\n` : '')
}
