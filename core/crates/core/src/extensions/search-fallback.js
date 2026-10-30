// Included at the top of the bundled Pi extension. Pi's built-in find and grep
// run fd and ripgrep, which Pi downloads when they are missing. Offline, or
// where the download fails, the built-in tools return an error. This fallback
// answers those calls in process, with the same arguments and output format,
// and leaves every other result, including native fd and ripgrep output, as is.
import { lstat as searchLstat, open as searchOpen, readdir as searchReaddir, stat as searchStat } from 'node:fs/promises';
import searchPath from 'node:path';

const SEARCH_UNAVAILABLE = /^(fd is not available and could not be downloaded|ripgrep \(rg\) is not available and could not be downloaded|Failed to run (fd|ripgrep):)/;
const SEARCH_SKIPPED_DIRECTORIES = new Set(['.git', 'node_modules']);
const SEARCH_MAX_BYTES = 50 * 1024;
const SEARCH_MAX_LINE = 500;
const SEARCH_MAX_FILE_BYTES = 10 * 1024 * 1024;

// Converts a glob to an anchored expression over `/`-separated relative paths.
// `windows` also reads `\` as a separator, as a Windows path does.
function searchGlob(pattern, windows = searchPath.sep === '\\') {
  let glob = windows ? pattern.replace(/\\/g, '/') : pattern;
  let source = '';
  let braces = 0;
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === '*') {
      if (glob[i + 1] === '*') {
        const before = i === 0 || glob[i - 1] === '/';
        let end = i + 2;
        while (glob[end] === '*') end++;
        if (before && glob[end] === '/') { source += '(?:[^/]*/)*'; i = end; continue; }
        if (before && end === glob.length) { source += '.*'; i = end; continue; }
        source += '[^/]*'; i = end - 1; continue;
      }
      source += '[^/]*';
    } else if (c === '?') {
      source += '[^/]';
    } else if (c === '[') {
      const close = glob.indexOf(']', i + 2);
      if (close === -1) { source += '\\['; continue; }
      let set = glob.slice(i + 1, close).replace(/\\/g, '\\\\');
      if (set[0] === '!') set = '^' + set.slice(1);
      source += `[${set}]`;
      i = close;
    } else if (c === '{') {
      braces++; source += '(?:';
    } else if (c === '}' && braces > 0) {
      braces--; source += ')';
    } else if (c === ',' && braces > 0) {
      source += '|';
    } else {
      source += c.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    }
  }
  if (braces > 0) throw new Error(`Invalid glob pattern: ${pattern}`);
  return new RegExp(`^${source}$`, windows ? 'i' : '');
}

// The path the model gave, against the run's working directory.
function searchResolve(cwd, value) {
  let given = value || '.';
  if (given === '~' || given.startsWith('~/')) given = searchPath.join(process.env.HOME || process.env.USERPROFILE || '', given.slice(1));
  return searchPath.resolve(cwd, given);
}

function searchAborted(signal) {
  if (signal?.aborted) throw new Error('Operation aborted');
}

// Walks one directory tree. A symbolic link is listed but never followed, so
// the walk stays inside the tree and a link cycle cannot loop.
async function* searchWalk(root, signal) {
  const pending = [''];
  while (pending.length) {
    searchAborted(signal);
    const relative = pending.shift();
    let entries;
    try {
      entries = await searchReaddir(searchPath.join(root, relative), { withFileTypes: true });
    } catch (error) {
      if (!relative) throw error;
      continue;
    }
    entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    for (const entry of entries) {
      const path = relative ? `${relative}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        if (SEARCH_SKIPPED_DIRECTORIES.has(entry.name)) continue;
        yield { path, directory: true };
        pending.push(path);
      } else {
        yield { path, directory: false, file: entry.isFile() };
      }
    }
  }
}

function searchTruncate(lines) {
  let output = '';
  let truncated = false;
  for (const line of lines) {
    const next = output ? `${output}\n${line}` : line;
    if (Buffer.byteLength(next) > SEARCH_MAX_BYTES) { truncated = true; break; }
    output = next;
  }
  return { output, truncated };
}

async function searchFind(input, cwd, signal) {
  searchAborted(signal);
  const pattern = String(input.pattern ?? '');
  if (!pattern) throw new Error('A glob pattern is required.');
  const root = searchResolve(cwd, input.path);
  const info = await searchStat(root).catch(() => undefined);
  if (!info) throw new Error(`Path not found: ${root}`);
  if (!info.isDirectory()) throw new Error(`Not a directory: ${root}`);
  const limit = Math.max(1, Math.floor(Number(input.limit) || 1000));
  const windows = searchPath.sep === '\\';
  const normalized = windows ? pattern.replace(/\\/g, '/') : pattern;
  // As with fd, a pattern with a separator matches the whole relative path.
  const full = normalized.includes('/');
  const anchored = full && !normalized.startsWith('/') && !normalized.startsWith('**/') && normalized !== '**' ? `**/${normalized}` : normalized.replace(/^\//, '');
  const matcher = searchGlob(anchored, windows);
  const found = [];
  for await (const entry of searchWalk(root, signal)) {
    const name = entry.path.slice(entry.path.lastIndexOf('/') + 1);
    if (matcher.test(full ? entry.path : name)) found.push(entry.directory ? `${entry.path}/` : entry.path);
    if (found.length >= limit) break;
  }
  if (!found.length) return { content: [{ type: 'text', text: 'No files found matching pattern' }], details: undefined };
  const { output, truncated } = searchTruncate(found);
  const details = {};
  const notices = [];
  if (found.length >= limit) {
    notices.push(`${limit} results limit reached. Use limit=${limit * 2} for more, or refine pattern`);
    details.resultLimitReached = limit;
  }
  if (truncated) {
    notices.push(`${SEARCH_MAX_BYTES / 1024}KB limit reached`);
    details.truncation = { truncated: true, maxBytes: SEARCH_MAX_BYTES };
  }
  return { content: [{ type: 'text', text: notices.length ? `${output}\n[${notices.join('. ')}]` : output }], details: notices.length ? details : undefined };
}

// Reads a regular file as text. A file over the size bound or with a NUL byte
// in its first block is binary to ripgrep and is skipped.
async function searchReadText(file) {
  const info = await searchLstat(file).catch(() => undefined);
  if (!info?.isFile() || info.size > SEARCH_MAX_FILE_BYTES) return undefined;
  const handle = await searchOpen(file, 'r').catch(() => undefined);
  if (!handle) return undefined;
  try {
    const buffer = await handle.readFile();
    if (buffer.subarray(0, 8192).includes(0)) return undefined;
    return buffer.toString('utf8');
  } finally {
    await handle.close();
  }
}

async function searchGrep(input, cwd, signal) {
  searchAborted(signal);
  const pattern = String(input.pattern ?? '');
  if (!pattern) throw new Error('A search pattern is required.');
  const target = searchResolve(cwd, input.path);
  const info = await searchStat(target).catch(() => undefined);
  if (!info) throw new Error(`Path not found: ${target}`);
  let expression;
  try {
    const source = input.literal ? pattern.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') : pattern;
    expression = new RegExp(source, input.ignoreCase ? 'i' : '');
  } catch (error) {
    throw new Error(`Invalid search pattern: ${error.message}`);
  }
  const filter = input.glob ? searchGlob(input.glob) : undefined;
  const filterFull = input.glob ? (searchPath.sep === '\\' ? input.glob.replace(/\\/g, '/') : input.glob).includes('/') : false;
  const context = input.context > 0 ? Math.floor(input.context) : 0;
  const limit = Math.max(1, Math.floor(Number(input.limit) || 100));
  const files = [];
  if (info.isDirectory()) {
    for await (const entry of searchWalk(target, signal)) {
      if (!entry.file) continue;
      const name = entry.path.slice(entry.path.lastIndexOf('/') + 1);
      if (filter && !filter.test(filterFull ? entry.path : name)) continue;
      files.push([searchPath.join(target, entry.path), entry.path]);
    }
  } else {
    files.push([target, searchPath.basename(target)]);
  }
  const output = [];
  let matches = 0;
  let linesTruncated = false;
  const clip = text => {
    if (text.length <= SEARCH_MAX_LINE) return text;
    linesTruncated = true;
    return `${text.slice(0, SEARCH_MAX_LINE)}... [truncated]`;
  };
  scan: for (const [file, shown] of files) {
    searchAborted(signal);
    const text = await searchReadText(file);
    if (!text) continue;
    const lines = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n');
    if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();
    for (let index = 0; index < lines.length; index++) {
      if (!expression.test(lines[index])) continue;
      matches++;
      const start = Math.max(0, index - context);
      const end = Math.min(lines.length - 1, index + context);
      for (let current = start; current <= end; current++) {
        output.push(current === index ? `${shown}:${current + 1}: ${clip(lines[current])}` : `${shown}-${current + 1}- ${clip(lines[current])}`);
      }
      if (matches >= limit) break scan;
    }
  }
  if (!matches) return { content: [{ type: 'text', text: 'No matches found' }], details: undefined };
  const { output: text, truncated } = searchTruncate(output);
  const details = {};
  const notices = [];
  if (matches >= limit) {
    notices.push(`${limit} matches limit reached. Use limit=${limit * 2} for more, or refine pattern`);
    details.matchLimitReached = limit;
  }
  if (truncated) {
    notices.push(`${SEARCH_MAX_BYTES / 1024}KB limit reached`);
    details.truncation = { truncated: true, maxBytes: SEARCH_MAX_BYTES };
  }
  if (linesTruncated) {
    notices.push(`Some lines truncated to ${SEARCH_MAX_LINE} chars. Use read tool to see full lines`);
    details.linesTruncated = true;
  }
  return { content: [{ type: 'text', text: notices.length ? `${text}\n[${notices.join('. ')}]` : text }], details: notices.length ? details : undefined };
}

// Replaces only the result of a find or grep call whose native search program
// could not run. Any failure of the fallback stays an error result.
function registerSearchFallback(pi) {
  // A loader that registers tools without events keeps the native results.
  if (typeof pi.on !== 'function') return;
  pi.on('tool_result', async (event, context) => {
    if ((event.toolName !== 'find' && event.toolName !== 'grep') || !event.isError) return undefined;
    const message = (event.content || []).map(part => (part.type === 'text' ? part.text : '')).join('');
    if (!SEARCH_UNAVAILABLE.test(message.trim())) return undefined;
    const search = event.toolName === 'find' ? searchFind : searchGrep;
    try {
      const result = await search(event.input || {}, context?.cwd || process.cwd(), context?.signal);
      return { ...result, isError: false };
    } catch (error) {
      return { content: [{ type: 'text', text: error instanceof Error ? error.message : String(error) }], details: undefined, isError: true };
    }
  });
}
