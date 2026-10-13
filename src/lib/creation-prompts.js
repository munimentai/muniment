export const creationOptions = {
  artifact: [
    {name:'Interactive report',goal:'Explain a topic with an interactive report',output:'A self-contained HTML report with clear sources and useful controls'},
    {name:'Dashboard',goal:'Build a dashboard for the data I provide',output:'An HTML dashboard with readable charts, filters, and a summary'},
    {name:'Calculator',goal:'Build a calculator for a decision or task',output:'An interactive HTML calculator with labeled inputs and clear results'},
  ],
  agent: [
    {name:'Research assistant',goal:'Research topics and compare reliable sources',output:'A reusable agent that produces a sourced research brief'},
    {name:'Writing assistant',goal:'Help draft and revise clear writing',output:'A reusable agent that produces drafts and explains key revisions'},
    {name:'Project reviewer',goal:'Review project files and identify actionable issues',output:'A reusable agent that produces a prioritized review with file references'},
  ],
}
export function creationDefaults(kind) {
  return kind === 'agent' ? {goal:'Create a reusable agent for my work',output:'A saved agent with a clear task, instructions, and expected output'} : {goal:'Create an interactive artifact',output:'A verified HTML artifact available in Muniment'}
}
export function creationPrompt(kind, plan) {
  return `I want to create ${kind === 'agent' ? 'an agent' : 'an artifact'} in this dedicated chat. Goal: ${plan.goal}. Expected output: ${plan.output}. Ask a few focused questions about any missing requirements, then build it and save it in Muniment. Keep this chat for future revisions.`
}
// The starters an empty catalog shows, and the row above a full one. Each opens
// a dedicated creation chat with its goal and output filled in.
export const creationStarters = {
  artifact: [
    {id:'report',icon:'file-text',name:'Report',sentence:'Explains a topic with sections, sources and charts.',goal:'Explain a topic with an interactive report',output:'A self-contained HTML report with clear sources and useful controls'},
    {id:'dashboard',icon:'layout-dashboard',name:'Dashboard',sentence:'Charts and numbers from the data you give it.',goal:'Build a dashboard for the data I provide',output:'An HTML dashboard with readable charts, filters, and a summary'},
    {id:'calculator',icon:'calculator',name:'Calculator',sentence:'Labeled inputs and a clear answer for a decision.',goal:'Build a calculator for a decision or task',output:'An interactive HTML calculator with labeled inputs and clear results'},
    {id:'tracker',icon:'list-checks',name:'Tracker',sentence:'A list you add to, filter and check off.',goal:'Build a tracker for a list I keep',output:'An interactive HTML tracker that adds, edits, filters and checks off items'},
    {id:'explainer',icon:'book-open',name:'Explainer',sentence:'Walks through a topic one step at a time.',goal:'Teach a topic step by step',output:'An interactive HTML explainer that walks through a topic in numbered steps'},
  ],
  agent: [
    {id:'morning-briefing',name:'Morning briefing',sentence:'Summarizes what changed in your work before you start.',badge:'Weekdays',goal:'Create an agent that writes me a morning briefing',output:'A saved agent that runs on weekday mornings and summarizes what changed in my projects and threads'},
    {id:'research-desk',name:'Research desk',sentence:'Answers with sources and says when it is unsure.',goal:'Research topics and answer with reliable sources',output:'A reusable agent that answers with cited sources and states what it could not confirm'},
    {id:'writing-editor',name:'Writing editor',sentence:'Revises your drafts in your voice and lists each change.',goal:'Edit my drafts in my own voice',output:'A reusable agent that revises drafts and lists each change it made'},
    {id:'weekly-report',name:'Weekly report',sentence:'Writes a recap of the week every Friday.',badge:'Weekly',goal:'Compile a weekly recap of my work',output:'A saved agent that runs every Friday and writes a recap from my threads and projects'},
    {id:'project-keeper',name:'Project keeper',sentence:'Tracks open items and asks before it acts.',goal:'Track the open items in a project',output:'A reusable agent that keeps a list of open items and asks before it changes anything'},
  ],
}
