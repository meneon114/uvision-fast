import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from '@codemirror/language';
import { openSearchPanel, search, searchKeymap } from '@codemirror/search';
import { Compartment, EditorState, RangeSet, StateEffect, StateField } from '@codemirror/state';
import {
  Decoration,
  DecorationSet,
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
import { ICONS } from './icons';

const armAsmLanguage = StreamLanguage.define({
  token(stream) {
    if (stream.eatSpace()) return null;
    if (stream.peek() === ';') {
      stream.skipToEnd();
      return 'comment';
    }
    if (stream.peek() === '"') {
      stream.next();
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === '"') break;
      }
      return 'string';
    }
    if (stream.match(/^#-?0x[0-9a-fA-F]+/i) || stream.match(/^#-?[0-9]+/) || stream.match(/^0x[0-9a-fA-F]+/i)) {
      return 'number';
    }
    if (stream.match(/^[0-9]+\b/)) {
      return 'number';
    }
    if (stream.match(/^[A-Za-z_.$][A-Za-z0-9_.$]*/)) {
      const w = stream.current().toUpperCase();
      if (ASM_DIRECTIVES.has(w)) return 'meta';
      if (ASM_REGS.has(w)) return 'variableName.special';
      if (ASM_MNEMONICS.has(w)) return 'keyword';
      return 'variableName';
    }
    stream.next();
    return null;
  },
});

const cSimpleLanguage = StreamLanguage.define({
  startState: () => ({ inComment: false }),
  token(stream, state) {
    if (state.inComment) {
      if (stream.match('*/')) {
        state.inComment = false;
      } else {
        stream.next();
      }
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
    if (stream.peek() === '"' || stream.peek() === '\'') {
      const q = stream.next();
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === '\\') {
          stream.next();
        } else if (ch === q) {
          break;
        }
      }
      return 'string';
    }
    if (stream.match(/^0x[0-9a-fA-F]+[uUlL]*/i) || stream.match(/^[0-9]+[uUlL]*/)) {
      return 'number';
    }
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

const keilHighlightStyle = HighlightStyle.define([
  { tag: tags.comment, color: '#008000' },
  { tag: tags.keyword, color: '#0000ff', fontWeight: 'bold' },
  { tag: tags.meta, color: '#800080', fontWeight: 'bold' },
  { tag: tags.typeName, color: '#0000ff' },
  { tag: tags.special(tags.variableName), color: '#006080', fontWeight: 'bold' },
  { tag: tags.number, color: '#800080' },
  { tag: tags.string, color: '#a31515' },
]);

const ASM_DIRECTIVES = new Set([
  'AREA', 'EXPORT', 'IMPORT', 'EXTERN', 'DCD', 'DCW', 'DCB', 'SPACE', 'EQU',
  'ALIGN', 'ENTRY', 'END', 'THUMB', 'ARM', 'PRESERVE8', 'REQUIRE8', 'PROC', 'ENDP',
  'DATA', 'CODE', 'READONLY', 'READWRITE', 'NOINIT', 'GET', 'INCLUDE', 'MACRO', 'MEND',
]);

const ASM_REGS = new Set([
  'R0', 'R1', 'R2', 'R3', 'R4', 'R5', 'R6', 'R7', 'R8', 'R9', 'R10', 'R11', 'R12',
  'R13', 'R14', 'R15', 'SP', 'LR', 'PC', 'PSR', 'XPSR', 'CPSR', 'MSP', 'PSP', 'PRIMASK',
]);

const ASM_MNEMONICS = new Set([
  'MOV', 'MOVS', 'MOVW', 'MOVT', 'MVN', 'MVNS', 'ADD', 'ADDS', 'ADC', 'ADCS', 'ADR',
  'SUB', 'SUBS', 'SBC', 'SBCS', 'RSB', 'RSBS', 'MUL', 'MULS', 'MLA', 'MLS', 'UDIV', 'SDIV',
  'UMULL', 'SMULL', 'UMLAL', 'SMLAL', 'AND', 'ANDS', 'ORR', 'ORRS', 'ORN', 'EOR', 'EORS',
  'BIC', 'BICS', 'CMP', 'CMN', 'TST', 'TEQ', 'LSL', 'LSLS', 'LSR', 'LSRS', 'ASR', 'ASRS',
  'ROR', 'RORS', 'RRX', 'LDR', 'LDRB', 'LDRSB', 'LDRH', 'LDRSH', 'LDRD', 'STR', 'STRB',
  'STRH', 'STRD', 'LDM', 'LDMIA', 'LDMDB', 'STM', 'STMIA', 'STMDB', 'PUSH', 'POP',
  'B', 'BL', 'BX', 'BLX', 'BEQ', 'BNE', 'BCS', 'BHS', 'BCC', 'BLO', 'BMI', 'BPL',
  'BVS', 'BVC', 'BHI', 'BLS', 'BGE', 'BLT', 'BGT', 'BLE', 'BAL', 'CBZ', 'CBNZ',
  'IT', 'ITE', 'ITT', 'ITEE', 'ITTE', 'ITTT', 'SVC', 'BKPT', 'NOP', 'WFI', 'WFE',
  'UXTB', 'UXTH', 'SXTB', 'SXTH', 'CLZ', 'RBIT', 'REV', 'REV16', 'REVSH', 'BFI', 'BFC', 'UBFX', 'SBFX',
  'MRS', 'MSR', 'CPSID', 'CPSIE', 'DSB', 'DMB', 'ISB', 'TBB', 'TBH',
]);

const C_KEYWORDS = new Set([
  'if', 'else', 'for', 'while', 'do', 'switch', 'case', 'default', 'break', 'continue',
  'return', 'goto', 'sizeof', 'typedef', 'struct', 'union', 'enum', 'const', 'volatile',
  'static', 'extern', 'inline', 'register', '__attribute__',
]);

const C_TYPES = new Set([
  'void', 'char', 'short', 'int', 'long', 'float', 'double', 'signed', 'unsigned',
  'uint8_t', 'uint16_t', 'uint32_t', 'uint64_t', 'int8_t', 'int16_t', 'int32_t', 'int64_t', 'bool',
]);

export interface GutterDebugState {
  pcLine: number | null;
  breakpoints: Set<number>;
  execLines: Set<number>;
}

const setDebugStateEffect = StateEffect.define<GutterDebugState>();

const debugStateField = StateField.define<GutterDebugState>({
  create: () => ({ pcLine: null, breakpoints: new Set(), execLines: new Set() }),
  update(val, tr) {
    for (const e of tr.effects) {
      if (e.is(setDebugStateEffect)) {
        return e.value;
      }
    }
    return val;
  },
});

const pcLineDeco = Decoration.line({ attributes: { class: 'uv-pc-line' } });

const pcHighlightField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(decos, tr) {
    for (const e of tr.effects) {
      if (e.is(setDebugStateEffect)) {
        if (e.value.pcLine && e.value.pcLine >= 1 && e.value.pcLine <= tr.state.doc.lines) {
          const line = tr.state.doc.line(e.value.pcLine);
          return Decoration.set([pcLineDeco.range(line.from)]);
        }
        return Decoration.none;
      }
    }
    return decos.map(tr.changes);
  },
  provide: (f) => EditorView.decorations.from(f),
});

class KeilGutterMarker extends GutterMarker {
  constructor(
    private readonly hasBp: boolean,
    private readonly isPc: boolean,
    private readonly isExec: boolean
  ) {
    super();
  }
  eq(other: KeilGutterMarker): boolean {
    return this.hasBp === other.hasBp && this.isPc === other.isPc && this.isExec === other.isExec;
  }
  toDOM(): HTMLElement {
    const wrap = document.createElement('div');
    wrap.className = 'uv-gutter-mark';
    if (this.hasBp) {
      const dot = document.createElement('span');
      dot.className = 'uv-bp-dot';
      wrap.appendChild(dot);
    } else if (this.isExec) {
      const bar = document.createElement('span');
      bar.className = 'uv-exec-bar';
      wrap.appendChild(bar);
    }
    if (this.isPc) {
      const arrow = document.createElement('span');
      arrow.style.position = 'absolute';
      arrow.style.left = '2px';
      arrow.style.width = '13px';
      arrow.style.height = '13px';
      arrow.innerHTML = ICONS.pcArrow;
      wrap.appendChild(arrow);
    }
    return wrap;
  }
}

export class KeilEditor {
  public readonly view: EditorView;
  private readonly langComp = new Compartment();
  private suppressChange = false;

  constructor(
    parent: HTMLElement,
    private readonly callbacks: {
      onChange: (content: string) => void;
      onToggleBreakpoint: (line: number) => void;
      onCursorMove: (line: number, col: number) => void;
      onSave: () => void;
    }
  ) {
    const bpGutter = gutter({
      class: 'uv-bp-gutter',
      markers(view) {
        const dbg = view.state.field(debugStateField);
        const builders: any[] = [];
        const lines = new Set<number>();
        dbg.breakpoints.forEach((l) => lines.add(l));
        dbg.execLines.forEach((l) => lines.add(l));
        if (dbg.pcLine) lines.add(dbg.pcLine);
        const sorted = Array.from(lines).sort((a, b) => a - b);
        for (const ln of sorted) {
          if (ln >= 1 && ln <= view.state.doc.lines) {
            const lineObj = view.state.doc.line(ln);
            builders.push(
              new KeilGutterMarker(
                dbg.breakpoints.has(ln),
                dbg.pcLine === ln,
                dbg.execLines.has(ln)
              ).range(lineObj.from)
            );
          }
        }
        return RangeSet.of(builders);
      },
      domEventHandlers: {
        mousedown(view, line) {
          const ln = view.state.doc.lineAt(line.from).number;
          callbacks.onToggleBreakpoint(ln);
          return true;
        },
      },
    });

    const saveKey = keymap.of([
      {
        key: 'Mod-s',
        run: () => {
          callbacks.onSave();
          return true;
        },
      },
    ]);

    const state = EditorState.create({
      doc: '',
      extensions: [
        debugStateField,
        pcHighlightField,
        bpGutter,
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightActiveLine(),
        drawSelection(),
        history(),
        search(),
        syntaxHighlighting(keilHighlightStyle),
        this.langComp.of(armAsmLanguage),
        saveKey,
        keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
        EditorView.updateListener.of((upd) => {
          if (upd.docChanged && !this.suppressChange) {
            callbacks.onChange(upd.state.doc.toString());
          }
          if (upd.selectionSet || upd.docChanged) {
            const head = upd.state.selection.main.head;
            const line = upd.state.doc.lineAt(head);
            callbacks.onCursorMove(line.number, head - line.from + 1);
          }
        }),
      ],
    });

    this.view = new EditorView({ state, parent });
  }

  public setDocument(content: string, isC: boolean): void {
    this.suppressChange = true;
    const lang = isC ? cSimpleLanguage : armAsmLanguage;
    this.view.dispatch({
      changes: { from: 0, to: this.view.state.doc.length, insert: content },
      effects: [
        this.langComp.reconfigure(lang),
        setDebugStateEffect.of({ pcLine: null, breakpoints: new Set(), execLines: new Set() }),
      ],
    });
    this.suppressChange = false;
  }

  public setDebugState(dbg: GutterDebugState): void {
    this.view.dispatch({
      effects: setDebugStateEffect.of(dbg),
    });
    if (dbg.pcLine && dbg.pcLine >= 1 && dbg.pcLine <= this.view.state.doc.lines) {
      this.scrollToLine(dbg.pcLine);
    }
  }

  public scrollToLine(lineNum: number): void {
    const clamped = Math.max(1, Math.min(lineNum, this.view.state.doc.lines));
    const line = this.view.state.doc.line(clamped);
    this.view.dispatch({
      selection: { anchor: line.from },
      effects: EditorView.scrollIntoView(line.from, { y: 'center' }),
    });
  }

  public getCurrentLine(): number {
    const head = this.view.state.selection.main.head;
    return this.view.state.doc.lineAt(head).number;
  }

  public openFind(): void {
    openSearchPanel(this.view);
  }
}
