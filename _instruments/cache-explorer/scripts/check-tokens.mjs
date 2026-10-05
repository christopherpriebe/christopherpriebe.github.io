// Two rules for the UI's sources, run through `npm run check` in web/ (or on another folder:
// `node check-tokens.mjs <dir>`):
//
// 1. Signal and ramp colours are fills and strokes, never type: no `color:` declaration, no
//    `.color =`, `setProperty('color', …)` or `setAttribute('color', …)` in a script, no `<text>`
//    fill in a stylesheet, no `<text>` element built with a `fill` attribute, and no fill set on a
//    variable that holds a `<text>` element may use them. The check reads the sources, so it cannot
//    see a class that a script puts on a text element, or a text element it cannot follow through a
//    variable in the same file; a stylesheet rule that fills `text` elements is caught, one that
//    fills a class is not.
// 2. No name of the design export's own token set appears: the tokens here are --cache-explorer-*.
import { readFileSync, readdirSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

const root = resolve(process.argv[2] ?? join(import.meta.dirname, '..', 'web', 'src'));
const SIGNAL = ['tag', 'index', 'offset', 'evict', 'writeback', 'writethrough', 'dirty', 'stale', 'correct', 'incorrect'];
const signalToken = new RegExp(`--cache-explorer-(?:${SIGNAL.join('|')})(?![\\w-])`);
const foreignToken = /--cl-/;

function* sources(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* sources(path);
    else if (/\.(css|ts)$/.test(entry.name)) yield path;
  }
}

const problems = [];
const report = (file, text, index, what) => {
  const line = text.slice(0, index).split('\n').length;
  problems.push(`${relative(process.cwd(), file)}:${line}: ${what}`);
};

for (const file of sources(root)) {
  const text = readFileSync(file, 'utf8');
  if (foreignToken.test(text)) report(file, text, text.search(foreignToken), 'a --cl-* token name; use --cache-explorer-*');

  // `color:` as a property (not background-color, border-color and the like), in a stylesheet or a style string.
  for (const m of text.matchAll(/(?<![\w-])color\s*:\s*([^;}`'"]*)/g)) {
    if (signalToken.test(m[1])) report(file, text, m.index, `${m[1].trim()} used as type colour`);
  }
  if (file.endsWith('.css')) {
    // A rule whose selector names `text` elements, with a fill that is a signal or ramp colour.
    for (const m of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      const onText = m[1].split(',').some((selector) => /(?:^|[\s>+~])text\s*$/.test(selector.trim()));
      const fill = /(?<![\w-])fill\s*:\s*([^;}]*)/.exec(m[2]);
      if (onText && fill && signalToken.test(fill[1])) report(file, text, m.index + m[0].search(/\S/), `${fill[1].trim()} used as the fill of text`);
    }
  } else {
    // s('text', { fill: … }): the attribute object is flat, so the first closing brace ends it.
    for (const m of text.matchAll(/\bs\(\s*['"]text['"]\s*,\s*\{([^}]*)\}/g)) {
      const fill = /(?<![\w-])fill\s*:\s*([^,}]*)/.exec(m[1]);
      if (fill && signalToken.test(fill[1])) report(file, text, m.index, `${fill[1].trim()} used as the fill of text`);
    }
    // The colour rule through the DOM: el.style.color = …, el.style.setProperty('color', …), el.setAttribute('color', …).
    for (const m of text.matchAll(/(?:\.color\s*=(?!=)|(?:setProperty|setAttribute)\(\s*['"]color['"]\s*,)\s*([^;\n]*)/g)) {
      if (signalToken.test(m[1])) report(file, text, m.index, `${m[1].trim()} used as type colour`);
    }
    // A variable that holds a text element (t = s('text', …), t = document.createElementNS(…, 'text')), then its fill set on it.
    for (const [, name] of text.matchAll(/\b(\w+)(?:\s*:\s*[\w.<>]+)?\s*=\s*(?:s\(\s*['"]text['"]|document\.createElementNS\([^)]*['"]text['"])/g)) {
      const setsFill = new RegExp(`\\b${name}\\.(?:setAttribute\\(\\s*['"]fill['"]\\s*,|style\\.fill\\s*=(?!=)|style\\.setProperty\\(\\s*['"]fill['"]\\s*,)\\s*([^;\\n]*)`, 'g');
      for (const m of text.matchAll(setsFill)) {
        if (signalToken.test(m[1])) report(file, text, m.index, `${m[1].trim()} used as the fill of text`);
      }
    }
  }
}

if (problems.length > 0) {
  console.error(`check-tokens: signal and ramp colours are never type, and the design's token names are not used:\n${problems.join('\n')}`);
  process.exit(1);
}
