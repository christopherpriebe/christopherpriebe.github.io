// The address strip: the address in binary, one block for each field in proportion to its bits,
// with the field's colour under it, and under each field a swatch, its name and value, and a note
// such as `4 bits → word 1, byte 0`. The bit row is SVG, as the wires that leave it are; the
// labels are HTML, so they wrap when a field is narrow. Both sit in the same grid, whose columns
// are the fields' bit counts, so a label is always exactly as wide as the blocks above it.

import type { FieldKind, Geometry } from '../../pkg/cache_explorer';
import { copy } from '../copy';
import { cls, h, s } from '../dom';
import type { AddressSource } from './source';

/** The height of a bit block, and of the colour under it. */
const BIT_HEIGHT = 34;
const RULE_HEIGHT = 6;

interface Field {
  field: FieldKind;
  bits: string;
  /** What the field is worth, and the note under it; empty while there is no address. */
  valueText: string;
  note: string;
  /** How many bits the field has, which sets its share of the strip. */
  width: number;
}

/** The fields to draw: the decoded address's, or the geometry's, with blank blocks, when there is none. */
function fieldsOf(source: AddressSource, geometry: Geometry): Field[] {
  if (source.decoded) {
    return source.decoded.fields.map(({ field, bits, valueText, note }) => ({
      field,
      bits,
      valueText,
      note,
      width: bits.length,
    }));
  }
  const shape: [FieldKind, number][] = [
    ['tag', geometry.tagBits],
    ['index', geometry.indexBits],
    ['offset', geometry.offsetBits],
  ];
  return shape
    .filter(([, width]) => width > 0)
    .map(([field, width]) => ({ field, bits: ' '.repeat(width), valueText: '', note: '', width }));
}

const percent = (part: number, whole: number): string => `${String((part / whole) * 100)}%`;

/** One field's row of bit blocks, in a coordinate system of its own width. */
function bitRow(field: Field, blank: boolean): SVGSVGElement {
  const { bits, width } = field;
  const digits = Array.from(bits);
  return s(
    'svg',
    {
      class: 'cache-explorer__bits',
      width: '100%',
      height: BIT_HEIGHT + RULE_HEIGHT,
      'data-fld': field.field,
      role: blank ? null : 'img',
      'aria-label': blank ? null : `${copy.strip.fields[field.field]} ${bits}`,
      'aria-hidden': blank ? 'true' : null,
    },
    ...digits.map((_, i) =>
      s('rect', {
        class: 'cache-explorer__bit',
        x: percent(i, width),
        y: 0,
        width: percent(1, width),
        height: BIT_HEIGHT,
      }),
    ),
    s('rect', {
      class: `cache-explorer__bit-rule cache-explorer__bit-rule--${field.field}`,
      x: 0,
      y: BIT_HEIGHT,
      width: '100%',
      height: RULE_HEIGHT,
    }),
    // The one-pixel seams between blocks show the plane through the block and its colour.
    ...digits.slice(1).map((_, i) =>
      s('rect', {
        class: 'cache-explorer__bit-seam',
        x: percent(i + 1, width),
        y: 0,
        width: 1,
        height: BIT_HEIGHT + RULE_HEIGHT,
        transform: 'translate(-0.5 0)',
      }),
    ),
    ...(blank
      ? []
      : digits.map((digit, i) =>
          s(
            'text',
            {
              class: 'cache-explorer__bit-digit',
              x: percent(i + 0.5, width),
              y: BIT_HEIGHT / 2,
            },
            digit,
          ),
        )),
  );
}

function label(field: Field): HTMLElement {
  const name = copy.strip.fields[field.field];
  return h(
    'div',
    { class: 'cache-explorer__field-label' },
    h('span', {
      class: `cache-explorer__swatch cache-explorer__swatch--${field.field}`,
      'aria-hidden': 'true',
    }),
    h(
      'b',
      { class: 'cache-explorer__field-value' },
      field.valueText === '' ? name : `${name} ${field.valueText}`,
    ),
    field.note !== '' && h('span', { class: 'cache-explorer__field-note' }, field.note),
  );
}

/** The strip, with its footer: the address at the left and where it came from at the right. */
export function addressStrip(source: AddressSource, geometry: Geometry): HTMLElement {
  const fields = fieldsOf(source, geometry);
  const blank = source.decoded === null;
  return h(
    'div',
    { class: cls('cache-explorer__strip', blank && 'cache-explorer__strip--blank') },
    h(
      'div',
      {
        class: 'cache-explorer__fields',
        style: `grid-template-columns: ${fields.map((f) => `minmax(0, ${String(f.width)}fr)`).join(' ')}`,
      },
      ...fields.map((field) =>
        h('div', { class: 'cache-explorer__field-col' }, bitRow(field, blank), label(field)),
      ),
    ),
    h(
      'div',
      { class: 'cache-explorer__strip-foot' },
      h(
        'span',
        { class: 'cache-explorer__label cache-explorer__label--strong' },
        `${copy.strip.address} ${source.decoded?.addrText ?? copy.strip.noAddress}`,
      ),
      h('span', { class: 'cache-explorer__label' }, source.footer),
    ),
  );
}
