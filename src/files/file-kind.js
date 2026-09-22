export function fileKind(name, directory = false) {
  if (directory) return { icon: 'folder', text: false }
  const extension = name.split('.').at(-1).toLowerCase()
  if (['png','jpg','jpeg','gif','webp','svg','heic','ico','avif'].includes(extension)) return { icon: 'image', text: false }
  if (['mp3','wav','m4a','flac','ogg'].includes(extension)) return { icon: 'music', text: false }
  if (['mp4','mov','mkv','webm'].includes(extension)) return { icon: 'video', text: false }
  if (['csv','tsv','xlsx','xls','ods'].includes(extension)) return { icon: 'sheet', text: ['csv','tsv'].includes(extension) }
  if (['zip','gz','tar','7z','dmg'].includes(extension)) return { icon: 'archive', text: false }
  if (['js','mjs','cjs','ts','tsx','jsx','rs','py','go','sh','css','html','json','toml','yaml','yml','xml','sql'].includes(extension)) return { icon: 'file-code', text: true }
  if (['pdf','doc','docx','ppt','pptx'].includes(extension)) return { icon: 'file-text', text: false }
  return { icon: 'file-text', text: true }
}
