// MunimentAvatar: the Muniment brand kit's avatar generator (avatars/m/robots.js).
// The drawing code below is that file's, unchanged except for the ES module
// wrapper, so every client draws the same avatar for a seed. Update it only by
// copying the brand file again.

/* muniment avatars: deterministic robot head SVGs in the muniment palette.
   MunimentAvatar.svg(seed, opts) takes an index or any string.
   Indexes 0..TOTAL-1 each map to a different head shape. A string hashes to an index for the
   shape and seeds the theme separately, so two names rarely share a whole avatar.
   The heads borrow the brand's materials (ink, paper, verdigris, folds, seals, ledger rules)
   but never the Pocket Fold geometry, because the mark itself carries no face. */

const LIGHT = { signal: '#2A7264', oxide: '#B4483E', ochre: '#B98A2F' };
const DARK = { signal: '#58B39F', oxide: '#C96A61', ochre: '#CBA14E' };
const THEMES = [
  { name: 'ink', weight: 38, bgs: ['#F8F7F4', '#EEEDE8', '#FFFFFF'], head: '#111111', fc: '#F8F7F4', accent: LIGHT.signal, marks: LIGHT },
  { name: 'reverse', weight: 27, bgs: ['#141716', '#1C201E'], head: '#E8EBE9', fc: '#141716', accent: DARK.signal, marks: DARK },
  { name: 'outline', weight: 15, bgs: ['#FFFFFF', '#F8F7F4'], head: '#FFFFFF', stroke: '#111111', fc: '#111111', accent: LIGHT.signal, marks: LIGHT },
  { name: 'signal', weight: 10, bgs: ['#F8F7F4', '#EEEDE8'], head: '#2A7264', fc: '#F8F7F4', accent: '#111111', marks: LIGHT },
  { name: 'graphite', weight: 10, bgs: ['#EEEDE8', '#141716'], head: '#5C6461', fc: '#F8F7F4', accent: DARK.signal, marks: DARK },
];
const THEME_WEIGHT = THEMES.reduce((n, t) => n + t.weight, 0);

const DIMS = {
  head: ['block', 'wide', 'tall', 'panel', 'arch', 'tablet'],
  top: ['none', 'stalk', 'ball', 'fold', 'tab', 'twin'],
  eyes: ['block', 'dash', 'dot', 'bar', 'visor', 'lens', 'arc', 'odd', 'rings', 'frame', 'scan', 'focus'],
  mouth: ['none', 'line', 'ledger', 'grille', 'arc', 'slot'],
  sides: ['none', 'bolts', 'ears', 'tabs'],
  mark: ['none', 'seal', 'barcode', 'rivets', 'brow'],
};
const KEYS = Object.keys(DIMS);
const TOTAL = KEYS.reduce((n, k) => n * DIMS[k].length, 1);
// STRIDE shares no factor with TOTAL (2^7 * 3^4 * 5), so i -> i * STRIDE mod TOTAL is a bijection
// that spreads neighboring indexes across every trait.
const STRIDE = 7919;

// [x, y, width, height, corner radii [top-left as [rx, ry], top-right, bottom-right, bottom-left]].
const HEADS = {
  block: [30, 36, 60, 60, [[3, 3], 3, 3, 3]],
  wide: [20, 44, 80, 52, [[3, 3], 3, 3, 3]],
  tall: [34, 30, 52, 70, [[3, 3], 3, 3, 3]],
  panel: [28, 36, 64, 62, [[10, 10], 10, 10, 10]],
  arch: [28, 34, 64, 64, [[30, 26], 3, 3, 3]],
  tablet: [30, 34, 60, 64, [[3, 3], 3, 16, 16]],
};

function hash(str) {
  let h = 2166136261;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function rng(seed) {
  let a = seed >>> 0;
  return function () {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function normalize(seed) {
  return String(seed).trim().toLowerCase();
}

function toIndex(seed) {
  if (typeof seed === 'number') return ((Math.floor(seed) % TOTAL) + TOTAL) % TOTAL;
  return hash(normalize(seed)) % TOTAL;
}

function traits(seed) {
  const index = toIndex(seed);
  let k = (index * STRIDE + 4242) % TOTAL;
  const t = { index };
  for (const key of KEYS) {
    const list = DIMS[key];
    t[key] = list[k % list.length];
    k = Math.floor(k / list.length);
  }
  const r = rng(typeof seed === 'number' ? index * 2654435761 + 1 : hash('theme:' + normalize(seed)));
  let w = r() * THEME_WEIGHT;
  const theme = THEMES.find((th) => (w -= th.weight) < 0) || THEMES[0];
  t.theme = theme.name;
  t.bg = theme.bgs[Math.floor(r() * theme.bgs.length)];
  const seals = Object.values(theme.marks).filter((c) => c !== theme.head);
  t.seal = seals[Math.floor(r() * seals.length)];
  return t;
}

const n = (v) => Math.round(v * 10) / 10;
const rect = (x, y, w, h, a) => `<rect x="${n(x)}" y="${n(y)}" width="${n(w)}" height="${n(h)}" ${a}/>`;
const circle = (cx, cy, r, a) => `<circle cx="${n(cx)}" cy="${n(cy)}" r="${r}" ${a}/>`;
const line = (x1, y1, x2, y2, a) => `<line x1="${n(x1)}" y1="${n(y1)}" x2="${n(x2)}" y2="${n(y2)}" ${a}/>`;
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

// Clockwise outline from the top-left corner. A fold replaces the top-right corner with a diagonal cut.
function headPath(x, y, w, h, radii, fold) {
  const X = x + w, Y = y + h;
  const [[tlx, tly], tr, br, bl] = radii;
  let d = `M${x + tlx} ${y}`;
  d += fold ? `H${n(X - fold)}L${X} ${n(y + fold)}` : `H${X - tr}A${tr} ${tr} 0 0 1 ${X} ${y + tr}`;
  d += `V${Y - br}A${br} ${br} 0 0 1 ${X - br} ${Y}`;
  d += `H${x + bl}A${bl} ${bl} 0 0 1 ${x} ${Y - bl}`;
  d += `V${y + tly}A${tlx} ${tly} 0 0 1 ${x + tlx} ${y}Z`;
  return d;
}

function top(kind, x, y, w, cx, body, accent) {
  const stalk = (sx, len) => line(sx, y + 1, sx, y - len, `stroke="${body}" stroke-width="3"`);
  const A = `fill="${accent}"`;
  switch (kind) {
    case 'stalk': return stalk(cx, 9) + rect(cx - 5, y - 19, 10, 10, A + ' rx="2"');
    case 'ball': return stalk(cx, 9) + circle(cx, y - 14, 5, A);
    case 'twin':
      return stalk(cx - 10, 8) + rect(cx - 13.5, y - 15, 7, 7, A + ' rx="1.5"') +
        stalk(cx + 10, 8) + rect(cx + 6.5, y - 15, 7, 7, A + ' rx="1.5"');
    case 'tab': {
      const x0 = x + w * 0.56, x1 = x + w - 5;
      return `<path d="M${n(x0)} ${y + 1}V${y - 5}A3 3 0 0 1 ${n(x0 + 3)} ${y - 8}H${n(x1 - 3)}A3 3 0 0 1 ${n(x1)} ${y - 5}V${y + 1}Z" ${A}/>`;
    }
    default: return '';
  }
}

function sides(kind, x, y, w, h, silhouette, theme) {
  const my = y + h * 0.5, X = x + w;
  switch (kind) {
    case 'bolts': return rect(x - 5, my - 5, 7, 10, silhouette + ' rx="1"') + rect(X - 2, my - 5, 7, 10, silhouette + ' rx="1"');
    case 'ears': return rect(x - 8, my - 11, 10, 22, silhouette + ' rx="3"') + rect(X - 2, my - 11, 10, 22, silhouette + ' rx="3"');
    case 'tabs':
      return [theme.marks.ochre, theme.marks.oxide, theme.accent]
        .map((c, i) => rect(X - 2, y + h * (0.3 + i * 0.18), 7, 8, `fill="${c}" rx="1.5"`)).join('');
    default: return '';
  }
}

function eyes(kind, x, w, cx, ey, dx, fc, accent) {
  const F = `fill="${fc}"`;
  const S = (sw) => `fill="none" stroke="${fc}" stroke-width="${sw}"`;
  const both = (f) => f(cx - dx) + f(cx + dx);
  const block = (ex) => rect(ex - 5, ey - 5, 10, 10, F + ' rx="1.5"');
  switch (kind) {
    case 'block': return both(block);
    case 'dash': return both((ex) => rect(ex - 6, ey - 1.75, 12, 3.5, F));
    case 'dot': return both((ex) => circle(ex, ey, 4.5, F));
    case 'bar': return both((ex) => rect(ex - 2.5, ey - 7, 5, 14, F + ' rx="1"'));
    case 'visor': {
      const vw = w * 0.6;
      return rect(cx - vw / 2, ey - 4.5, vw, 9, F + ' rx="2"') + rect(cx + vw / 2 - 12, ey - 2, 8, 4, `fill="${accent}"`);
    }
    case 'lens': return circle(cx, ey, 8.5, S(3.5)) + circle(cx, ey, 3.5, `fill="${accent}"`);
    case 'arc': return both((ex) => `<path d="M${n(ex - 6)} ${n(ey + 3)}A6 6 0 0 1 ${n(ex + 6)} ${n(ey + 3)}" ${S(3.5)}/>`);
    case 'odd': return block(cx - dx) + circle(cx + dx, ey, 3.5, F);
    case 'rings': return both((ex) => circle(ex, ey, 5, S(3)));
    case 'frame': return both((ex) => rect(ex - 5.5, ey - 6.5, 11, 13, S(2.5)) + rect(ex - 1.5, ey - 1.5, 3, 3, F));
    case 'scan': return line(x + 5, ey, x + w - 5, ey, `stroke="${accent}" stroke-width="2"`) + both(block);
    case 'focus': return both((ex) => block(ex) + rect(ex - 2, ey - 2, 4, 4, `fill="${accent}"`));
    default: return '';
  }
}

function mouth(kind, cx, my, fc, accent) {
  const F = `fill="${fc}"`;
  switch (kind) {
    case 'line': return rect(cx - 9, my - 1.5, 18, 3, F);
    case 'ledger': return rect(cx - 11, my - 3, 22, 2, F) + rect(cx - 11, my + 1, 22, 2, F);
    case 'grille': return [-6, 0, 6].map((o) => rect(cx + o - 1.5, my - 4.5, 3, 9, F)).join('');
    case 'arc': return `<path d="M${n(cx - 8)} ${n(my - 2)}A8 8 0 0 0 ${n(cx + 8)} ${n(my - 2)}" fill="none" stroke="${fc}" stroke-width="3.5"/>`;
    case 'slot': return rect(cx - 12, my - 2.5, 24, 5, F + ' rx="1"') + rect(cx + 2, my - 2.5, 10, 5, `fill="${accent}" rx="1"`);
    default: return '';
  }
}

function mark(kind, x, y, w, h, cx, ey, dx, fc, seal) {
  const X = x + w, Y = y + h;
  switch (kind) {
    case 'seal': return circle(X - 11, Y - 11, 4.5, `fill="${seal}"`);
    case 'barcode': {
      const bars = [1.5, 1, 2.5, 1, 1.5, 2, 1];
      const total = bars.reduce((s, b) => s + b, 0) + 1.2 * (bars.length - 1);
      let bx = cx - total / 2;
      return bars.map((b) => { const r = rect(bx, Y - 8, b, 4, `fill="${fc}"`); bx += b + 1.2; return r; }).join('');
    }
    case 'rivets': return circle(x + 7, Y - 7, 2, `fill="${fc}"`) + circle(X - 7, Y - 7, 2, `fill="${fc}"`);
    case 'brow': return rect(cx - dx - 4, ey - 14, 2 * dx + 8, 2.5, `fill="${fc}"`);
    default: return '';
  }
}

function svg(seed, opts) {
  opts = opts || {};
  const t = traits(seed);
  const theme = THEMES.find((th) => th.name === t.theme);
  const size = opts.size || 120;
  const [x, y, w, h, radii] = HEADS[t.head];
  const cx = x + w / 2, ey = y + h * 0.44, my = y + h * 0.7, dx = Math.min(w * 0.19, 12);
  const fold = t.top === 'fold' ? Math.round(Math.min(w, h) * 0.22) : 0;
  const body = theme.stroke || theme.head;
  const silhouette = `fill="${theme.head}"` + (theme.stroke ? ` stroke="${theme.stroke}" stroke-width="3" stroke-linejoin="round"` : '');

  const parts = [];
  if (opts.background !== false) parts.push(`<rect width="120" height="120" rx="${opts.radius || 0}" fill="${t.bg}"/>`);
  parts.push(sides(t.sides, x, y, w, h, silhouette, theme));
  parts.push(top(t.top, x, y, w, cx, body, theme.accent));
  parts.push(`<path d="${headPath(x, y, w, h, radii, fold)}" ${silhouette}/>`);
  if (fold) {
    const g = theme.stroke ? 4.5 : 3.5;
    parts.push(`<path d="M${n(x + w - fold)} ${n(y + g)}V${n(y + fold)}H${n(x + w - g)}Z" fill="${theme.accent}"/>`);
  }
  parts.push(eyes(t.eyes, x, w, cx, ey, dx, theme.fc, theme.accent));
  parts.push(mouth(t.mouth, cx, my, theme.fc, theme.accent));
  parts.push(mark(t.mark, x, y, w, h, cx, ey, dx, theme.fc, t.seal));

  const label = esc(opts.label || (typeof seed === 'number' ? 'Avatar ' + t.index : seed));
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120" width="${size}" height="${size}" role="img" aria-label="${label}">${parts.join('')}</svg>`;
}

export { svg, traits, TOTAL, DIMS, THEMES }
