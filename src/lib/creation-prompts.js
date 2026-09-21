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
