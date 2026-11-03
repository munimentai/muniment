// The questions a `muniment:ask_user_question` gate carries and the answer it
// takes. The gate's prefill holds `{ questions }`, and the answer is the
// editor value `{ answers }`, one entry per question with its picked options
// and text.
export const QUESTION_TITLE = 'muniment:ask_user_question'

export function isQuestionGate(gate) {
  return gate?.kind === 'editor' && gate.title === QUESTION_TITLE
}

// The questions in a gate's prefill, or `[]` when the payload is not a valid
// list of one to four questions with unique ids and at most six options each.
export function parseQuestions(payload) {
  try {
    const value = JSON.parse(payload).questions
    if (!Array.isArray(value) || value.length < 1 || value.length > 4) return []
    if (new Set(value.map(q => q.id)).size !== value.length) return []
    return value.every(q => typeof q.id === 'string' && typeof q.question === 'string'
      && (!q.options || (Array.isArray(q.options) && q.options.length <= 6 && q.options.every(o => typeof o.label === 'string')))) ? value : []
  } catch { return [] }
}

export function questionsComplete(questions, answers) {
  return questions.length > 0 && questions.every(q => answers[q.id]?.selected?.length || answers[q.id]?.text?.trim())
}

// The answers with one option picked or unpicked. A single-choice question
// keeps one option.
export function pickOption(answers, question, label) {
  const current = { selected: [], text: '', ...answers[question.id] }
  const selected = question.multiSelect
    ? (current.selected.includes(label) ? current.selected.filter(x => x !== label) : [...current.selected, label])
    : [label]
  return { ...answers, [question.id]: { ...current, selected } }
}

// The editor value the gate takes.
export function questionAnswerValue(questions, answers) {
  return JSON.stringify({ answers: questions.map(q => ({ id: q.id, question: q.question, selected: [], text: '', ...answers[q.id] })) })
}
