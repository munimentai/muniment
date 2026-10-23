// RobotAvatar: the Muniment brand kit's avatar generator (avatars/robots.js).
// The lines below the header comment are that file's, unchanged and with
// its indentation, inside an ES module wrapper in place of its script
// wrapper, so every client draws the same avatar for a seed. Update it only
// by copying the brand file again.

/* Boxbot avatars: deterministic robot head SVGs.
   RobotAvatar.svg(seed, opts) takes an index or any string. opts.tone is 'light' (default) or
   'dark'. Both tones draw the same robot with the same head colors on a matching tile.
   Indexes 0..TOTAL-1 each map to a different head shape. A string hashes to an index for the
   shape and seeds the colors separately, so two names rarely share a whole robot. */

  const INKS = ['#141414', '#141414', '#141414', '#3b3835'];
  const FILLS = ['#ffffff', '#f2b0ea', '#a9e38f', '#f4d36c', '#9fd6f2', '#f8b994', '#c6b6f5', '#8fe3cf', '#f6a0a0', '#ece6d6'];
  const BGS = ['#d3f1f8', '#c6eab2', '#fbf9f3', '#f9dcf4', '#fff0c4', '#e4ddfb', '#ffe1d1', '#d9f5ec'];
  const CHEEK = '#f27fb8';
  // Dark tone: each light background maps to a deep tile of the same hue, and the outline,
  // shadow and outside strokes switch from ink to this light line so they read on the tile.
  const DARK_BGS = {
    '#d3f1f8': '#13262b', '#c6eab2': '#17251a', '#fbf9f3': '#1c1b18', '#f9dcf4': '#2a1a27',
    '#fff0c4': '#29240f', '#e4ddfb': '#1f1b2e', '#ffe1d1': '#2b1d16', '#d9f5ec': '#142822',
  };
  const DARK_LINE = '#f3f1ec';
  // How far each antenna reaches above the head top, including its stroke.
  const ANTENNA_REACH = { square: 23.5, ball: 23.5, none: 2, twin: 21.5, bolt: 27.5, dish: 16.5 };

  const DIMS = {
    head: ['square', 'wide', 'tall', 'rounded', 'chamfer', 'dome', 'trapezoid'],
    antenna: ['square', 'ball', 'none', 'twin', 'bolt', 'dish'],
    eyes: ['block', 'dash', 'dot', 'bar', 'visor', 'happy', 'wince', 'cyclops', 'odd', 'cross', 'ring', 'glasses'],
    mouth: ['none', 'line', 'grille', 'smile', 'o', 'teeth'],
    sides: ['none', 'bolts', 'ears', 'phones'],
    extra: ['none', 'cheeks', 'sparkle', 'screws'],
  };
  const KEYS = Object.keys(DIMS);
  const TOTAL = KEYS.reduce((n, k) => n * DIMS[k].length, 1);
  // STRIDE shares no factor with TOTAL (2^8 * 3^3 * 7), so i -> i * STRIDE mod TOTAL is a bijection
  // that spreads neighboring indexes across every trait.
  const STRIDE = 7919;

  // [x, y, width, height] of each head in the 120x120 view box.
  const BOXES = {
    square: [28, 36, 64, 64],
    wide: [20, 44, 80, 54],
    tall: [34, 30, 52, 72],
    rounded: [28, 36, 64, 64],
    chamfer: [28, 36, 64, 64],
    dome: [28, 36, 64, 64],
    trapezoid: [26, 38, 68, 62],
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
    const r = rng(typeof seed === 'number' ? index * 2654435761 + 1 : hash('color:' + normalize(seed)));
    const pick = (list) => list[Math.floor(r() * list.length)];
    t.fill = pick(FILLS);
    t.accent = pick(FILLS);
    t.bg = pick(BGS);
    t.ink = pick(INKS);
    t.style = r() < 0.78 ? 'shadow' : 'inverse';
    return t;
  }

  const n = (v) => Math.round(v * 10) / 10;
  const rect = (x, y, w, h, a) => `<rect x="${n(x)}" y="${n(y)}" width="${n(w)}" height="${n(h)}" ${a}/>`;
  const circle = (cx, cy, r, a) => `<circle cx="${n(cx)}" cy="${n(cy)}" r="${r}" ${a}/>`;
  const line = (x1, y1, x2, y2, a) => `<line x1="${n(x1)}" y1="${n(y1)}" x2="${n(x2)}" y2="${n(y2)}" ${a}/>`;
  const poly = (pts, a) => `<polyline points="${pts.map((p) => n(p[0]) + ',' + n(p[1])).join(' ')}" ${a}/>`;
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  function headPath(kind, x, y, w, h) {
    const X = x + w, Y = y + h;
    if (kind === 'rounded') {
      const r = 14;
      return `M${x + r} ${y}H${X - r}A${r} ${r} 0 0 1 ${X} ${y + r}V${Y - r}A${r} ${r} 0 0 1 ${X - r} ${Y}H${x + r}A${r} ${r} 0 0 1 ${x} ${Y - r}V${y + r}A${r} ${r} 0 0 1 ${x + r} ${y}Z`;
    }
    if (kind === 'chamfer') {
      const c = 12;
      return `M${x + c} ${y}H${X - c}L${X} ${y + c}V${Y - c}L${X - c} ${Y}H${x + c}L${x} ${Y - c}V${y + c}Z`;
    }
    if (kind === 'dome') {
      const r = 24;
      return `M${x} ${Y}V${y + r}A${r} ${r} 0 0 1 ${x + r} ${y}H${X - r}A${r} ${r} 0 0 1 ${X} ${y + r}V${Y}Z`;
    }
    if (kind === 'trapezoid') return `M${x + 8} ${y}H${X - 8}L${X} ${Y}H${x}Z`;
    return `M${x} ${y}H${X}V${Y}H${x}Z`;
  }

  function antenna(kind, cx, top, ink, accent) {
    const stalk = (x1, y1, x2, y2) => line(x1, y1, x2, y2, `stroke="${ink}" stroke-width="4"`);
    const tip = `fill="${accent}" stroke="${ink}" stroke-width="3"`;
    switch (kind) {
      case 'square':
        return stalk(cx, top, cx, top - 10) + rect(cx - 6, top - 22, 12, 12, tip);
      case 'ball':
        return stalk(cx, top, cx, top - 10) + circle(cx, top - 16, 6, tip);
      case 'twin':
        return stalk(cx - 10, top, cx - 16, top - 12) + rect(cx - 20, top - 20, 8, 8, tip) +
          stalk(cx + 10, top, cx + 16, top - 12) + rect(cx + 12, top - 20, 8, 8, tip);
      case 'bolt':
        return poly([[cx, top], [cx - 5, top - 6], [cx + 5, top - 12], [cx, top - 18]],
          `fill="none" stroke="${ink}" stroke-width="4" stroke-linejoin="miter"`) + circle(cx, top - 22, 4, tip);
      case 'dish':
        return stalk(cx, top, cx, top - 8) + `<path d="M${cx - 11} ${top - 15}A11 8 0 0 0 ${cx + 11} ${top - 15}Z" ${tip}/>`;
      default:
        return '';
    }
  }

  function sides(kind, x, y, w, h, inset, ink, accent) {
    const my = y + h * 0.5;
    const a = `fill="${accent}" stroke="${ink}" stroke-width="3"`;
    const L = x + inset, R = x + w - inset;
    switch (kind) {
      case 'bolts':
        return rect(L - 7, my - 6, 9, 12, a) + rect(R - 2, my - 6, 9, 12, a);
      case 'ears':
        return rect(L - 10, my - 12, 12, 24, a) + rect(R - 2, my - 12, 12, 24, a);
      case 'phones':
        return `<path d="M${n(L - 4)} ${n(my)}A${n((R - L) / 2 + 4)} ${n(h / 2 + 10)} 0 0 1 ${n(R + 4)} ${n(my)}" fill="none" stroke="${ink}" stroke-width="5"/>` +
          rect(L - 11, my - 13, 13, 26, a + ' rx="3"') + rect(R - 2, my - 13, 13, 26, a + ' rx="3"');
      default:
        return '';
    }
  }

  function eyes(kind, cx, ey, dx, w, fc, glint) {
    const F = `fill="${fc}"`;
    const S = (sw) => `fill="none" stroke="${fc}" stroke-width="${sw}"`;
    const G = `fill="${glint}"`;
    const both = (f) => f(cx - dx, -1) + f(cx + dx, 1);
    switch (kind) {
      case 'block': return both((ex) => rect(ex - 6, ey - 6, 12, 12, F));
      case 'dash': return both((ex) => rect(ex - 7, ey - 2, 14, 4, F));
      case 'dot': return both((ex) => circle(ex, ey, 5, F));
      case 'bar': return both((ex) => rect(ex - 3, ey - 8, 6, 16, F));
      case 'visor': {
        const vw = w * 0.64;
        return rect(cx - vw / 2, ey - 7, vw, 14, F) + both((ex) => rect(ex - 4, ey - 3, 8, 6, G));
      }
      case 'happy':
        return both((ex) => poly([[ex - 6, ey + 3], [ex, ey - 3], [ex + 6, ey + 3]], S(4) + ' stroke-linejoin="miter"'));
      case 'wince':
        return both((ex, s) => poly([[ex + 4 * s, ey - 6], [ex - 4 * s, ey], [ex + 4 * s, ey + 6]], S(4) + ' stroke-linejoin="miter"'));
      case 'cyclops': return rect(cx - 10, ey - 10, 20, 20, F) + rect(cx - 4, ey - 4, 8, 8, G);
      case 'odd': return rect(cx - dx - 7, ey - 7, 14, 14, F) + rect(cx + dx - 4, ey - 4, 8, 8, F);
      case 'cross':
        return both((ex) => `<path d="M${n(ex - 5)} ${n(ey - 5)}L${n(ex + 5)} ${n(ey + 5)}M${n(ex + 5)} ${n(ey - 5)}L${n(ex - 5)} ${n(ey + 5)}" ${S(4)}/>`);
      case 'ring': return both((ex) => circle(ex, ey, 5.5, S(4)));
      case 'glasses':
        return both((ex) => rect(ex - 8, ey - 7, 16, 14, S(3)) + rect(ex - 2.5, ey - 2.5, 5, 5, F)) +
          line(cx - dx + 8, ey - 2, cx + dx - 8, ey - 2, `stroke="${fc}" stroke-width="3"`);
      default: return '';
    }
  }

  function mouth(kind, cx, my, fc, hc) {
    const F = `fill="${fc}"`;
    switch (kind) {
      case 'line': return rect(cx - 10, my - 2, 20, 4, F);
      case 'grille': return [-9, -3, 3, 9].map((o) => rect(cx + o - 1.5, my - 5, 3, 10, F)).join('');
      case 'smile':
        return poly([[cx - 10, my - 3], [cx - 6, my + 3], [cx + 6, my + 3], [cx + 10, my - 3]],
          `fill="none" stroke="${fc}" stroke-width="4" stroke-linejoin="miter"`);
      case 'o': return rect(cx - 4, my - 4, 8, 8, `fill="none" stroke="${fc}" stroke-width="3"`);
      case 'teeth': {
        const s = `stroke="${fc}" stroke-width="2.5"`;
        return rect(cx - 12, my - 5, 24, 10, `fill="${hc}" stroke="${fc}" stroke-width="3"`) +
          line(cx - 4, my - 5, cx - 4, my + 5, s) + line(cx + 4, my - 5, cx + 4, my + 5, s);
      }
      default: return '';
    }
  }

  function sparkle(x, y, w, h, withSides, ink) {
    const a = `stroke="${ink}" stroke-width="4"`;
    let out = line(x - 14, y - 8, x - 6, y, a) + line(x + w + 14, y - 8, x + w + 6, y, a);
    if (!withSides) out += line(x - 18, y + h * 0.5, x - 8, y + h * 0.5, a) + line(x + w + 8, y + h * 0.5, x + w + 18, y + h * 0.5, a);
    return out;
  }

  function svg(seed, opts) {
    opts = opts || {};
    const t = traits(seed);
    const size = opts.size || 120;
    const inverse = t.style === 'inverse';
    const dark = opts.tone === 'dark';
    // ink draws the outline, shadow and everything outside the head. Features keep t.ink in both tones.
    const ink = dark ? DARK_LINE : t.ink;
    const bg = dark ? DARK_BGS[t.bg] : t.bg;
    const hc = inverse ? t.ink : t.fill;
    const fc = inverse ? t.fill : t.ink;
    const glint = t.accent !== fc ? t.accent : hc;
    const [x, y, w, h] = BOXES[t.head];
    const cx = x + w / 2, ey = y + h * 0.42, my = y + h * 0.72, dx = Math.min(w * 0.18, 13);
    const inset = t.head === 'trapezoid' ? 4 : 0;
    const d = headPath(t.head, x, y, w, h);

    const parts = [];
    if (t.extra === 'sparkle') parts.push(sparkle(x, y, w, h, t.sides !== 'none', ink));
    parts.push(`<path d="${d}" transform="translate(5 5)" fill="${inverse ? t.accent : ink}"/>`);
    parts.push(sides(t.sides, x, y, w, h, inset, ink, t.accent));
    parts.push(antenna(t.antenna, cx, y, ink, t.accent));
    parts.push(`<path d="${d}" fill="${hc}" stroke="${ink}" stroke-width="4" stroke-linejoin="miter"/>`);
    parts.push(eyes(t.eyes, cx, ey, dx, w, fc, glint));
    parts.push(mouth(t.mouth, cx, my, fc, hc));
    if (t.extra === 'cheeks') {
      parts.push(rect(cx - dx - 14, ey + 9, 10, 4, `fill="${CHEEK}" opacity=".75"`) + rect(cx + dx + 4, ey + 9, 10, 4, `fill="${CHEEK}" opacity=".75"`));
    }
    if (t.extra === 'screws') parts.push(circle(x + 12, y + 10, 2.5, `fill="${fc}"`) + circle(x + w - 12, y + 10, 2.5, `fill="${fc}"`));

    const label = esc(opts.label || (typeof seed === 'number' ? 'Robot ' + t.index : seed));
    // Center the drawing vertically: from the highest stroke above the head to the bottom of the shadow.
    const reach = Math.max(2, ANTENNA_REACH[t.antenna], t.sides === 'phones' ? 12.5 : 0, t.extra === 'sparkle' ? 9.5 : 0);
    const dy = n(60 - (y - reach + y + h + 5) / 2);
    const tile = opts.background !== false ? `<rect width="120" height="120" rx="${opts.radius || 0}" fill="${bg}"/>` : '';
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120" width="${size}" height="${size}" role="img" aria-label="${label}">${tile}<g transform="translate(0 ${dy})">${parts.join('')}</g></svg>`;
  }

export { svg, traits, TOTAL, DIMS }
