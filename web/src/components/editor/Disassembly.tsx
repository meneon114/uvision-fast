import { Fragment, useEffect, useMemo, useRef } from 'react';
import { hex32, normPath } from '../../lib/format';
import { toggleBreakpointAddr } from '../../state/ide';
import { useIde } from '../../state/store';
import { Icon } from '../Icon';

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
    <section className="group dgroup" aria-label="Disassembly">
      <div className="tabs">
        <div className="tab-wrap" data-active>
          <span className="tab">
            <Icon name="file-binary" className="fi" />
            <span>Disassembly</span>
          </span>
        </div>
      </div>
      <div className="code mono disasm" ref={list} role="region" aria-label="Disassembly listing" tabIndex={0}>
        {disasm.map((d) => {
          const key = d.file && d.line ? `${d.file}:${d.line}` : '';
          const showSrc = key && key !== lastKey;
          lastKey = key || lastKey;
          const isPc = pc !== undefined && d.addr === pc;
          const isBp = bpSet.has(d.addr);
          return (
            <Fragment key={d.addr}>
              {d.label ? <div className="dl lbl">{d.label}:</div> : null}
              {showSrc ? (
                <div className="dl src">
                  <span className="gut" />
                  <span className="tx">
                    {String(d.line).padStart(4)}: {(srcLine(d.file, d.line) ?? '').trim().replace(/\s+/g, ' ')}
                  </span>
                </div>
              ) : null}
              <div className="dl" data-pc={isPc || undefined}>
                <button
                  type="button"
                  className="gut"
                  aria-label={`${isBp ? 'Remove' : 'Set'} breakpoint at ${hex32(d.addr)}`}
                  onClick={() => void toggleBreakpointAddr(d.addr)}
                >
                  {isBp ? <span className="uv-bp" /> : null}
                  {isPc ? <Icon name="debug-stackframe" className="uv-pc" /> : null}
                </button>
                <span className="tx">
                  <span className="t-a">{hex32(d.addr)}</span> <span className="t-o">{d.opcode.padEnd(8)}</span> <span className="t-k">{d.mnemonic}</span>
                </span>
              </div>
            </Fragment>
          );
        })}
      </div>
    </section>
  );
}
