export function creationDefaults(kind) {
  return kind === 'agent' ? {goal:'Create a reusable agent for my work',output:'A saved agent with a clear task, instructions, and expected output'} : {goal:'Create an interactive artifact',output:'A verified HTML artifact available in Muniment'}
}
// The message a creation chat sends first. The composer shows the template as
// a chip, so `details` holds only what the user typed beside it.
export function creationPrompt(kind, plan, details = '') {
  const prompt = `I want to create ${kind === 'agent' ? 'an agent' : 'an artifact'} in this dedicated chat. Goal: ${plan.goal}. Expected output: ${plan.output}. Ask a few focused questions about any missing requirements, then build it and save it in Muniment. Keep this chat for future revisions.`
  return details.trim() ? `${prompt}\n\n${details.trim()}` : prompt
}

const SENT = /^I want to create (an agent|an artifact) in this dedicated chat\. Goal: (.+?)\. Expected output: (.+?)\. Ask a few focused questions about any missing requirements, then build it and save it in Muniment\. Keep this chat for future revisions\.(?:\n\n([\s\S]*))?$/

// A sent creation message read back as its template: the kind, the goal,
// the output and the user's own details. Any other text gives null.
export function sentCreation(text) {
  const match = SENT.exec(String(text ?? '').trim())
  if (!match) return null
  const kind = match[1] === 'an agent' ? 'agent' : 'artifact'
  const plan = { kind, goal: match[2], output: match[3] }
  return { ...plan, name: creationName(kind, plan), details: match[4]?.trim() ?? '' }
}

// The short name a creation shows: its starter's name, else New agent or New artifact.
export function creationName(kind, plan) {
  return creationStarters[kind]?.find((starter) => starter.goal === plan?.goal)?.name ?? (kind === 'agent' ? 'New agent' : 'New artifact')
}
// The starters an empty catalog shows, and the row above a full one. Each opens
// a dedicated creation chat with its goal and output filled in. `tint` names
// the starter tint its icon takes.
export const creationStarters = {
  artifact: [
    {id:'report',tint:'blue',icon:'file-text',name:'Report',sentence:'Explains a topic with sections, sources and charts.',goal:'Explain a topic with an interactive report',output:'A self-contained HTML report with clear sources and useful controls'},
    {id:'dashboard',tint:'green',icon:'layout-dashboard',name:'Dashboard',sentence:'Charts and numbers from the data you give it.',goal:'Build a dashboard for the data I provide',output:'An HTML dashboard with readable charts, filters, and a summary'},
    {id:'calculator',tint:'amber',icon:'calculator',name:'Calculator',sentence:'Labeled inputs and a clear answer for a decision.',goal:'Build a calculator for a decision or task',output:'An interactive HTML calculator with labeled inputs and clear results'},
    {id:'tracker',tint:'teal',icon:'list-checks',name:'Tracker',sentence:'A list you add to, filter and check off.',goal:'Build a tracker for a list I keep',output:'An interactive HTML tracker that adds, edits, filters and checks off items'},
    {id:'explainer',tint:'rose',icon:'book-open',name:'Explainer',sentence:'Walks through a topic one step at a time.',goal:'Teach a topic step by step',output:'An interactive HTML explainer that walks through a topic in numbered steps'},
  ],
  agent: [
    {id:'morning-briefing',name:'Morning briefing',sentence:'Summarizes what changed in your work before you start.',badge:'Weekdays',goal:'Create an agent that writes me a morning briefing',output:'A saved agent that runs on weekday mornings and summarizes what changed in my projects and threads'},
    {id:'research-desk',name:'Research desk',sentence:'Answers with sources and says when it is unsure.',goal:'Research topics and answer with reliable sources',output:'A reusable agent that answers with cited sources and states what it could not confirm'},
    {id:'writing-editor',name:'Writing editor',sentence:'Revises your drafts in your voice and lists each change.',goal:'Edit my drafts in my own voice',output:'A reusable agent that revises drafts and lists each change it made'},
    {id:'weekly-report',name:'Weekly report',sentence:'Writes a recap of the week every Friday.',badge:'Weekly',goal:'Compile a weekly recap of my work',output:'A saved agent that runs every Friday and writes a recap from my threads and projects'},
    {id:'project-keeper',name:'Project keeper',sentence:'Tracks open items and asks before it acts.',goal:'Track the open items in a project',output:'A reusable agent that keeps a list of open items and asks before it changes anything'},
  ],
}

// The starters a new thread shows in place of an empty transcript. `kind` picks
// the flow: a creation chat, a prompt the composer starts with, or the file
// picker with a prompt for the files it adds.
export const threadStarters = [
  {id:'page',tint:'blue',icon:'file-code',name:'Make a page',kind:'artifact',goal:'Make a web page',output:'A self-contained HTML page with clear sections'},
  {id:'research',tint:'green',icon:'search',name:'Research a topic',kind:'prompt',prompt:'Research this topic and cite your sources: '},
  {id:'summarize',tint:'amber',icon:'file-text',name:'Summarize a file',kind:'files',prompt:'Summarize this file.'},
  {id:'agent',tint:'rose',icon:'bot',name:'Set up an agent',kind:'agent'},
]

// The starters an empty Projects catalog shows. Each opens the new project
// form with its name filled in.
export const projectStarters = [
  {id:'client',tint:'blue',icon:'users',name:'Client work'},
  {id:'research',tint:'green',icon:'book-open',name:'Research'},
  {id:'personal',tint:'amber',icon:'house',name:'Personal'},
]
