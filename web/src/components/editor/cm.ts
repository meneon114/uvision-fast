import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from '@codemirror/language';
import { highlightSelectionMatches, search, searchKeymap } from '@codemirror/search';
import { Annotation, Compartment, EditorState, RangeSet, StateEffect, StateField, type Extension, type Range } from '@codemirror/state';
import {
  Decoration,
  type DecorationSet,
  EditorView,
  GutterMarker,
  drawSelection,
  gutter,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from '@codemirror/view';
import { tags } from '@lezer/highlight';

const ASM_DIRECTIVES = new Set([
  'AREA',
  'EXPORT',
  'IMPORT',
  'EXTERN',
  'GLOBAL',
  'DCD',
  'DCW',
  'DCB',
  'SPACE',
  'FILL',
  'EQU',
  'RN',
  'ALIGN',
  'ENTRY',
  'END',
  'THUMB',
  'ARM',
  'PRESERVE8',
  'REQUIRE8',
  'PROC',
  'ENDP',
  'LTORG',
  'DATA',
  'CODE',
  'READONLY',
  'READWRITE',
  'NOINIT',
  'GET',
  'INCLUDE',
  'MACRO',
  'MEND',
]);

const ASM_REGS = new Set([
  'R0',
  'R1',
  'R2',
  'R3',
  'R4',
  'R5',
  'R6',
  'R7',
  'R8',
  'R9',
  'R10',
  'R11',
  'R12',
  'R13',
  'R14',
  'R15',
  'SP',
  'LR',
  'PC',
  'PSR',
  'APSR',
  'XPSR',
  'MSP',
  'PSP',
  'PRIMASK',
  'CONTROL',
]);

const ASM_MNEMONICS = new Set([
  'MOV',
  'MOVS',
  'MOVW',
  'MOVT',
  'MVN',
  'MVNS',
  'ADD',
  'ADDS',
  'ADC',
  'ADCS',
  'ADR',
  'ADDW',
  'SUBW',
  'SUB',
  'SUBS',
  'SBC',
  'SBCS',
  'RSB',
  'RSBS',
  'MUL',
  'MULS',
  'MLA',
  'MLS',
  'UDIV',
  'SDIV',
  'UMULL',
  'SMULL',
  'UMLAL',
  'SMLAL',
  'AND',
  'ANDS',
  'ORR',
  'ORRS',
  'ORN',
  'EOR',
  'EORS',
  'BIC',
  'BICS',
  'CMP',
  'CMN',
  'TST',
  'TEQ',
  'LSL',
  'LSLS',
  'LSR',
  'LSRS',
  'ASR',
  'ASRS',
  'ROR',
  'RORS',
  'RRX',
  'LDR',
  'LDRB',
  'LDRSB',
  'LDRH',
  'LDRSH',
  'LDRD',
  'STR',
  'STRB',
  'STRH',
  'STRD',
  'LDM',
  'LDMIA',
  'LDMFD',
  'LDMDB',
  'STM',
  'STMIA',
  'STMDB',
  'STMFD',
  'PUSH',
  'POP',
  'B',
  'BL',
  'BX',
  'BLX',
  'BEQ',
  'BNE',
  'BCS',
  'BHS',
  'BCC',
  'BLO',
  'BMI',
  'BPL',
  'BVS',
  'BVC',
  'BHI',
  'BLS',
  'BGE',
  'BLT',
  'BGT',
  'BLE',
  'BAL',
  'CBZ',
  'CBNZ',
  'IT',
  'ITE',
  'ITT',
  'ITEE',
  'ITTE',
  'ITTT',
  'ITET',
  'SVC',
  'BKPT',
  'NOP',
  'WFI',
  'WFE',
  'SEV',
  'UXTB',
  'UXTH',
  'SXTB',
  'SXTH',
  'CLZ',
  'RBIT',
  'REV',
  'REV16',
  'REVSH',
  'BFI',
  'BFC',
  'UBFX',
  'SBFX',
  'MRS',
  'MSR',
  'CPSID',
  'CPSIE',
  'DSB',
  'DMB',
  'ISB',
  'TBB',
  'TBH',
]);

const C_KEYWORDS = new Set([
  'if',
  'else',
  'for',
  'while',
  'do',
  'switch',
  'case',
  'default',
  'break',
  'continue',
  'return',
  'goto',
  'sizeof',
  'typedef',
  'struct',
  'union',
  'enum',
  'const',
  'volatile',
  'static',
  'extern',
  'inline',
  'register',
  '__attribute__',
]);

const C_TYPES = new Set([
  'void',
  'char',
  'short',
  'int',
  'long',
  'float',
  'double',
  'signed',
  'unsigned',
  'uint8_t',
  'uint16_t',
  'uint32_t',
  'uint64_t',
  'int8_t',
  'int16_t',
  'int32_t',
  'int64_t',
  'bool',
]);

const armAsm = StreamLanguage.define<{ col0: boolean }>({
  startState: () => ({ col0: true }),
  token(stream, state) {
    if (stream.sol()) state.col0 = true;
    if (stream.eatSpace()) {
      state.col0 = false;
      return null;
    }
    const atCol0 = state.col0;
    state.col0 = false;
    if (stream.peek() === ';') {
      stream.skipToEnd();
      return 'comment';
    }
    if (stream.peek() === '"') {
      stream.next();
      while (!stream.eol()) if (stream.next() === '"') break;
      return 'string';
    }
    if (stream.match(/^#-?0x[0-9a-f]+/i) || stream.match(/^#-?[0-9]+/) || stream.match(/^0x[0-9a-f]+/i) || stream.match(/^[0-9]+\b/)) {
      return 'number';
    }
    if (stream.match(/^=[A-Za-z_][A-Za-z0-9_]*/)) return 'string';
    if (stream.match(/^\|[^|]*\|/)) return 'meta';
    if (stream.match(/^[A-Za-z_.$][A-Za-z0-9_.$]*/)) {
      const w = stream.current().toUpperCase();
      if (ASM_DIRECTIVES.has(w)) return 'meta';
      if (ASM_REGS.has(w)) return 'variableName.special';
      if (ASM_MNEMONICS.has(w) || ASM_MNEMONICS.has(w.replace(/\.(W|N)$/, ''))) return 'keyword';
      return atCol0 ? 'labelName' : 'variableName';
    }
    stream.next();
    return null;
  },
});

const cLang = StreamLanguage.define<{ inComment: boolean }>({
  startState: () => ({ inComment: false }),
  token(stream, state) {
    if (state.inComment) {
      if (stream.match('*/')) state.inComment = false;
      else stream.next();
      return 'comment';
    }
    if (stream.eatSpace()) return null;
    if (stream.match('//')) {
      stream.skipToEnd();
      return 'comment';
    }
    if (stream.match('/*')) {
      state.inComment = true;
      return 'comment';
    }
    if (stream.peek() === '#') {
      stream.skipToEnd();
      return 'meta';
    }
    if (stream.peek() === '"' || stream.peek() === "'") {
      const q = stream.next();
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === '\\') stream.next();
        else if (ch === q) break;
      }
      return 'string';
    }
    if (stream.match(/^0x[0-9a-f]+[ul]*/i) || stream.match(/^[0-9]+[ul]*/i)) return 'number';
    if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
      const w = stream.current();
      if (C_TYPES.has(w)) return 'typeName';
      if (C_KEYWORDS.has(w)) return 'keyword';
      return 'variableName';
    }
    stream.next();
    return null;
  },
});

// Colours come from the theme tokens, so light and dark follow the app theme.
const highlight = HighlightStyle.define([
  { tag: tags.comment, color: 'var(--t-c)', fontStyle: 'italic' },
  { tag: tags.keyword, color: 'var(--t-k)' },
  { tag: tags.meta, color: 'var(--t-d)' },
  { tag: tags.typeName, color: 'var(--t-k)' },
  { tag: tags.special(tags.variableName), color: 'var(--t-r)' },
  { tag: tags.labelName, color: 'var(--t-l)' },
  { tag: tags.number, color: 'var(--t-n)' },
  { tag: tags.string, color: 'var(--t-s)' },
]);

export interface GutterState {
  pcLine: number | null;
  breakpoints: ReadonlySet<number>;
  execLines: ReadonlySet<number>;
}

const EMPTY: GutterState = { pcLine: null, breakpoints: new Set(), execLines: new Set() };
export const setGutterState = StateEffect.define<GutterState>();

const gutterField = StateField.define<GutterState>({
  create: () => EMPTY,
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setGutterState)) return e.value;
    return v;
  },
});

const pcLineDeco = Decoration.line({ class: 'uv-pc-line' });

const pcField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(decos, tr) {
    for (const e of tr.effects) {
      if (e.is(setGutterState)) {
        const l = e.value.pcLine;
        if (l && l >= 1 && l <= tr.state.doc.lines) return Decoration.set([pcLineDeco.range(tr.state.doc.line(l).from)]);
        return Decoration.none;
      }
    }
    return decos.map(tr.changes);
  },
  provide: (f) => EditorView.decorations.from(f),
});

class Marker extends GutterMarker {
  constructor(
    readonly bp: boolean,
    readonly pc: boolean,
    readonly exec: boolean,
  ) {
    super();
  }
  eq(o: Marker): boolean {
    return o.bp === this.bp && o.pc === this.pc && o.exec === this.exec;
  }
  toDOM(): HTMLElement {
    const el = document.createElement('div');
    el.className = 'uv-mark';
    if (this.exec) el.appendChild(Object.assign(document.createElement('b'), { className: 'uv-exec' }));
    if (this.bp) el.appendChild(Object.assign(document.createElement('span'), { className: 'uv-bp' }));
    if (this.pc) {
      const a = document.createElement('i');
      a.className = 'codicon codicon-debug-stackframe uv-pc';
      a.setAttribute('aria-hidden', 'true');
      el.appendChild(a);
    }
    return el;
  }
}

export interface EditorCallbacks {
  onChange(content: string): void;
  onToggleBreakpoint(line: number): void;
  onCursor(line: number, col: number): void;
  onSave(): void;
}

export const external = Annotation.define<boolean>();
export const langCompartment = new Compartment();
export const readOnlyCompartment = new Compartment();

export function languageFor(path: string): Extension {
  return /\.(c|h)$/i.test(path) ? cLang : armAsm;
}

export function baseExtensions(cb: { current: EditorCallbacks }): Extension[] {
  const bpGutter = gutter({
    class: 'uv-bp-gutter',
    markers(view) {
      const g = view.state.field(gutterField);
      const lines = new Set<number>([...g.breakpoints, ...g.execLines]);
      if (g.pcLine) lines.add(g.pcLine);
      const out: Range<GutterMarker>[] = [];
      for (const ln of [...lines].sort((a, b) => a - b)) {
        if (ln < 1 || ln > view.state.doc.lines) continue;
        out.push(new Marker(g.breakpoints.has(ln), g.pcLine === ln, g.execLines.has(ln)).range(view.state.doc.line(ln).from));
      }
      return RangeSet.of(out);
    },
    initialSpacer: () => new Marker(true, false, false),
    domEventHandlers: {
      mousedown(view, block) {
        cb.current.onToggleBreakpoint(view.state.doc.lineAt(block.from).number);
        return true;
      },
    },
  });

  return [
    gutterField,
    pcField,
    bpGutter,
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightActiveLine(),
    drawSelection(),
    history(),
    search({ top: true }),
    highlightSelectionMatches(),
    syntaxHighlighting(highlight),
    EditorState.tabSize.of(8),
    keymap.of([{ key: 'Mod-s', run: () => (cb.current.onSave(), true) }, ...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
    EditorView.updateListener.of((u) => {
      if (u.docChanged && !u.transactions.some((t) => t.annotation(external))) cb.current.onChange(u.state.doc.toString());
      if (u.selectionSet || u.docChanged) {
        const head = u.state.selection.main.head;
        const line = u.state.doc.lineAt(head);
        cb.current.onCursor(line.number, head - line.from + 1);
      }
    }),
    EditorView.contentAttributes.of({ 'aria-label': 'Source editor' }),
  ];
}
