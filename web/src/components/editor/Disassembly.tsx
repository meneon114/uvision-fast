import { Fragment, useEffect, useMemo, useRef } from 'react';
import { hex32, normPath } from '../../lib/format';
import { toggleBreakpointAddr } from '../../state/ide';
import { useIde } from '../../state/store';
import { Icon } from '../Icon';
import { BP_MARK, GUT, GUT_HINT, PC_MARK } from './marks';
import { GROUP, TAB, TAB_BAR, TAB_WRAP } from './tabs';

const LINE = 'flex items-center h-[19px] whitespace-pre pr-3';
const TEXT = 'pl-1 overflow-hidden text-ellipsis';

export function Disassembly() {
  const disasm = useIde((s) => s.disasm);
  const pc = useIde((s) => s.dbg?.pc);
  const bps = useIde((s) => s.dbg?.breakpoints);
  const docs = useIde((s) => s.docs);
  const list = useRef<HTMLDivElement>(null);
  const bpSet = useMemo(() => new Set((bps ?? []).map((b) => b.addr)), [bps]);

  useEffect(() => {
    const el = list.current?.querySelector<HTMLElement>('[data-pc]');
    if (!el || !list.current) return;
    const box = list.current.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    if (r.top < box.top + 20 || r.bottom > box.bottom - 20) el.scrollIntoView({ block: 'center' });
  }, [pc]);

  const srcLine = (file: string | null, line: number | null) => {
    if (!file || !line) return null;
    const d = docs[normPath(file)] ?? Object.values(docs).find((x) => normPath(x.path).endsWith('/' + normPath(file).split('/').pop()));
    return d ? (d.content.split('\n')[line - 1]?.replace(/\s+$/, '') ?? '') : '';
  };

  let lastKey = '';
  return (
    <section className={GROUP} aria-label="Disassembly">
      <div className={TAB_BAR}>
        <div className={TAB_WRAP} data-active>
          <span className={`${TAB} pr-3`}>
            <Icon name="file-binary" className="text-asm text-md" />
            <span>Disassembly</span>
          </span>
        </div>
      </div>
      <div
        className="flex-1 overflow-auto py-1 bg-bg leading-[19px] min-h-0 font-mono text-ui"
        ref={list}
        role="region"
        aria-label="Disassembly listing"
        tabIndex={0}
      >
        {disasm.map((d) => {
          const key = d.file && d.line ? `${d.file}:${d.line}` : '';
          const showSrc = key && key !== lastKey;
          lastKey = key || lastKey;
          const isPc = pc !== undefined && d.addr === pc;
          const isBp = bpSet.has(d.addr);
          return (
            <Fragment key={d.addr}>
              {d.label ? <div className={`${LINE} pl-6.5 text-t-l`}>{d.label}:</div> : null}
              {showSrc ? (
                <div className={LINE}>
                  <span className={`${GUT} flex-none`} />
                  <span className={`${TEXT} text-muted`}>
                    {String(d.line).padStart(4)}: {(srcLine(d.file, d.line) ?? '').trim().replace(/\s+/g, ' ')}
                  </span>
                </div>
              ) : null}
              <div
                className={`${LINE} data-pc:bg-pc data-pc:shadow-[inset_0_1px_0_color-mix(in_srgb,var(--pcline)_60%,transparent),inset_0_-1px_0_color-mix(in_srgb,var(--pcline)_60%,transparent)]`}
                data-pc={isPc || undefined}
              >
                <button
                  type="button"
                  className={`${GUT} ${GUT_HINT} flex-none p-0 border-0 bg-transparent`}
                  aria-label={`${isBp ? 'Remove' : 'Set'} breakpoint at ${hex32(d.addr)}`}
                  onClick={() => void toggleBreakpointAddr(d.addr)}
                >
                  {isBp ? <span className={BP_MARK} data-bp /> : null}
                  {isPc ? <Icon name="debug-stackframe" className={PC_MARK} /> : null}
                </button>
                <span className={TEXT}>
                  <span className="text-t-a">{hex32(d.addr)}</span> <span className="text-t-o">{d.opcode.padEnd(8)}</span>{' '}
                  <span className="text-t-k">{d.mnemonic}</span>
                </span>
              </div>
            </Fragment>
          );
        })}
      </div>
    </section>
  );
}
