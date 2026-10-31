// Included in the bundled Pi extension. The desktop renders the editor payload
// as a questionnaire while Pi waits for its correlated RPC response.
const QUESTION_EXAMPLE = '{"questions":[{"id":"format","question":"Which format should the report use?","options":[{"label":"PDF"},{"label":"Spreadsheet","description":"An .xlsx file"}]}]}';
const QUESTION_GUIDANCE = `ask_user_question takes {"questions":[...]}, where each question has an "id", the "question" text and optional "options" with a "label" each. Example: ${QUESTION_EXAMPLE}`;
const QUESTION_KEYS = new Set(['id', 'question', 'multiSelect', 'options']);

// Rewrites the argument shapes local models send for one question into the
// canonical shape: a question without the `questions` list, `choices` for
// `options`, plain strings for options or questions, and a question without
// an id. Pi calls this before it checks the arguments against the schema, so
// the schema stays strict and any other shape fails with the guidance above.
function normalizeQuestionArguments(args) {
  if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error(QUESTION_GUIDANCE);
  let questions;
  let rest = args;
  if (Array.isArray(args.questions)) questions = args.questions;
  else if (args.questions && typeof args.questions === 'object') questions = [args.questions];
  else if (args.questions === undefined && (typeof args.question === 'string' || args.choices !== undefined || args.options !== undefined)) {
    const { questions: _questions, ...question } = args;
    questions = [question];
    rest = {};
  } else throw new Error(QUESTION_GUIDANCE);
  const canonical = questions === args.questions && questions.every(question => question && typeof question === 'object'
    && typeof question.id === 'string' && question.id && Object.keys(question).every(key => QUESTION_KEYS.has(key))
    && (question.options === undefined || (Array.isArray(question.options) && question.options.every(option => option && typeof option === 'object'))));
  if (canonical) return args;
  const given = new Set(questions.map(question => question?.id).filter(id => typeof id === 'string' && id));
  let next = 0;
  const freshId = () => {
    let id;
    do id = `question-${++next}`; while (given.has(id));
    given.add(id);
    return id;
  };
  const normalized = questions.map(item => {
    if (typeof item === 'string') item = { question: item };
    if (!item || typeof item !== 'object' || Array.isArray(item)) throw new Error(QUESTION_GUIDANCE);
    const { choices, ...question } = item;
    if (choices !== undefined) {
      if (question.options !== undefined) throw new Error(`Give a question either "options" or "choices", not both. ${QUESTION_GUIDANCE}`);
      question.options = choices;
    }
    if (Array.isArray(question.options)) {
      question.options = question.options.map(option => (typeof option === 'string' ? { label: option } : option));
    }
    if (question.id === undefined || question.id === '') question.id = freshId();
    for (const key of Object.keys(question)) {
      if (!QUESTION_KEYS.has(key)) throw new Error(`A question cannot have "${key}". ${QUESTION_GUIDANCE}`);
    }
    return question;
  });
  return { ...rest, questions: normalized };
}

function registerUserQuestion(pi) {
  pi.registerTool({
    name: 'ask_user_question',
    label: 'Ask a question',
    description: `Ask the user for missing requirements or a choice that affects the result. Use this tool instead of listing questions in chat. Ask only what cannot be inferred from the conversation or available files. The user can select choices or write an answer. This is not a permission or approval tool. Arguments example: ${QUESTION_EXAMPLE}`,
    parameters: {
      type: 'object', additionalProperties: false, required: ['questions'],
      properties: { questions: { type: 'array', minItems: 1, maxItems: 4, items: {
        type: 'object', additionalProperties: false, required: ['id', 'question'],
        properties: {
          id: { type: 'string', minLength: 1, maxLength: 80, description: 'A short unique id for the question, such as "format".' },
          question: { type: 'string', minLength: 1, maxLength: 2000, description: 'The question to show the user.' },
          multiSelect: { type: 'boolean', description: 'Whether the user may select more than one option.' },
          options: { type: 'array', maxItems: 6, description: 'Choices the user can select, each an object with a "label".', items: {
            type: 'object', additionalProperties: false, required: ['label'],
            properties: { label: { type: 'string', minLength: 1, maxLength: 200 }, description: { type: 'string', maxLength: 500 } }
          } }
        }
      } } }
    },
    prepareArguments: normalizeQuestionArguments,
    async execute(_id, args, signal, _update, context) {
      args = normalizeQuestionArguments(args);
      if (new Set(args.questions.map(q => q.id)).size !== args.questions.length) throw new Error('Question ids must be unique.');
      if (signal?.aborted) return { content: [{ type: 'text', text: 'The question was cancelled.' }] };
      let abort;
      const cancelled = new Promise(resolve => { abort = () => resolve(undefined); });
      signal?.addEventListener('abort', abort, { once: true });
      let value;
      try {
        value = await Promise.race([
          context.ui.editor('muniment:ask_user_question', JSON.stringify(args)), cancelled,
        ]);
      } finally {
        signal?.removeEventListener('abort', abort);
      }
      if (value === undefined || signal?.aborted) {
        return { content: [{ type: 'text', text: 'The user did not answer. Do not infer an answer.' }], details: { cancelled: true } };
      }
      const result = JSON.parse(value);
      if (!Array.isArray(result.answers) || result.answers.length !== args.questions.length) throw new Error('The answers are incomplete.');
      for (const question of args.questions) {
        const matches = result.answers.filter(answer => answer.id === question.id);
        const answer = matches[0];
        if (matches.length !== 1 || !Array.isArray(answer.selected) || typeof answer.text !== 'string'
          || (!answer.selected.length && !answer.text.trim())
          || (!question.multiSelect && answer.selected.length > 1)
          || answer.selected.some(label => !(question.options || []).some(option => option.label === label))) {
          throw new Error('The answer does not match the question.');
        }
      }
      return { content: [{ type: 'text', text: JSON.stringify(result) }], details: result };
    }
  });
}

registerUserQuestion(pi);
