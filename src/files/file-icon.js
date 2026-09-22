import theme from './seti/vs-seti-icon-theme.json'
const languages = {
  js:'javascript',mjs:'javascript',cjs:'javascript',jsx:'javascriptreact',ts:'typescript',tsx:'typescriptreact',
  py:'python',pyw:'python',rs:'rust',c:'c',cpp:'cpp',cc:'cpp',cs:'csharp',go:'go',rb:'ruby',php:'php',
  java:'java',json:'json',jsonc:'jsonc',jsonl:'jsonl',html:'html',htm:'html',css:'css',scss:'scss',less:'less',
  md:'markdown',mdx:'markdown',markdown:'markdown',sh:'shellscript',bash:'shellscript',zsh:'shellscript',
  fish:'shellscript',ps1:'powershell',bat:'bat',cmd:'bat',yml:'yaml',yaml:'yaml',xml:'xml',sql:'sql',db:'sql',
  sqlite:'sql',sqlite3:'sql',swift:'swift',lua:'lua',dart:'dart',jl:'julia',pl:'perl',tex:'latex',ini:'properties',
}
export function fileIcon(name) {
  const base = name.split(/[\\/]/).at(-1).toLowerCase()
  let id = theme.fileNames[base]
  if (!id) {
    const suffixes = base.split('.').slice(1).map((_,i) => base.split('.').slice(i+1).join('.'))
    id = suffixes.map(s => theme.fileExtensions[s]).find(Boolean)
  }
  if (!id) id = theme.languageIds[languages[base.split('.').at(-1)]]
  if (!id && /^(dockerfile|makefile)$/.test(base)) id = theme.languageIds[base]
  const dark = theme.iconDefinitions[id || theme.file]
  const light = theme.iconDefinitions[`${id || theme.file}_light`] || dark
  return { character:String.fromCodePoint(parseInt(dark.fontCharacter.replace('\\',''),16)), dark:dark.fontColor, light:light.fontColor }
}
