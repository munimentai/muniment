import * as monaco from 'monaco-editor/editor/editor.api'
import 'monaco-editor/basic-languages/monaco.contribution'
import 'monaco-editor/language/json/monaco.contribution'
import EditorWorker from 'monaco-editor/editor/editor.worker?worker'
import JsonWorker from 'monaco-editor/language/json/json.worker?worker'
globalThis.MonacoEnvironment = { getWorker: (_, label) => label === 'json' ? new JsonWorker() : new EditorWorker() }
for (const base of ['vs', 'vs-dark']) monaco.editor.defineTheme(`muniment-${base}`, {
  base, inherit: true, rules: [], colors: {
    'editor.selectionBackground': '#FFD84D',
    'editor.selectionForeground': '#171A18',
    'editor.inactiveSelectionBackground': '#FFD84D',
    'editor.selectionHighlightBackground': '#FFD84D55',
  },
})
export { monaco }
