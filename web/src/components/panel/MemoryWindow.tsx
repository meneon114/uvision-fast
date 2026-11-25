import { Input } from '../ui/Input';
import { cx } from '../../lib/cx';
import { CMD_WIN, Out } from './Out';
import { useEffect, useState } from 'react';
import { toast } from '@ui-sdk/react/toast';
import { hex32, hex8 } from '../../lib/format';
import { refreshMemory, setMemAddr, writeMemory } from '../../state/ide';
import { askText, useIde } from '../../state/store';

const ROW = 16;

export function MemoryWindow({ idx }: { idx: number }) {
  const addr = useIde((s) => s.memAddrs[idx]);
  const mem = useIde((s) => s.mem[idx]);
  const session = useIde((s) => s.session);
  const pc = useIde((s) => s.dbg?.pc);
  const [draft, setDraft] = useState(addr);
  useEffect(() => setDraft(addr), [addr]);
  useEffect(() => {
    if (session === 'on') void refreshMemory(idx);
  }, [session, idx, pc]);

  const changed = new Set(mem?.changed_offsets ?? []);
  const rows: number[] = [];
  if (mem) for (let o = 0; o < mem.bytes.length; o += ROW) rows.push(o);

  const edit = async (offset: number) => {
    if (!mem) return;
    const at = mem.addr + offset;
    const v = await askText(`Modify memory at ${hex32(at)}`, 'Byte value or space-separated bytes (hex)', hex8(mem.bytes[offset]));
    if (v === null) return;
    const bytes = v
      .split(/[\s,]+/)
      .filter(Boolean)
      .map((t) => parseInt(t.replace(/^0x/i, ''), 16));
    if (!bytes.length || bytes.some((b) => !Number.isInteger(b) || b < 0 || b > 255)) {
      toast.error('Enter bytes as hex, for example 3F or 01 02 FF');
      return;
    }
    await writeMemory(at, bytes, idx);
  };

  return (
    <div className={CMD_WIN}>
      <form
        className="h-7.5 flex items-center gap-2 px-3 border-b border-border flex-none text-sm text-muted"
        onSubmit={(e) => {
          e.preventDefault();
          if (draft.trim()) setMemAddr(idx, draft.trim());
        }}
      >
        <label htmlFor={`mem-addr-${idx}`}>Address:</label>
        <Input
          id={`mem-addr-${idx}`}
          mono
          compact
          className="w-45"
          aria-label={`Memory ${idx + 1} address`}
          value={draft}
          onValueChange={setDraft}
          placeholder="0x20000000, R13, GPIOC"
        />
      </form>
      {session !== 'on' ? (
        <Out muted>Memory windows read target memory while a debug session runs.</Out>
      ) : !mem ? (
        <Out muted>Reading {addr}...</Out>
      ) : (
        <Out mono className="leading-5" role="grid" aria-label={`Memory ${idx + 1}`} aria-readonly="false">
          {rows.map((o) => (
            <div className="flex gap-4.5 whitespace-pre" role="row" key={o}>
              <span className="text-t-a" role="rowheader">
                {hex32(mem.addr + o)}:
              </span>
              <span className="flex gap-0.5">
                {mem.bytes.slice(o, o + ROW).map((b, i) => (
                  <button
                    type="button"
                    role="gridcell"
                    key={i}
                    className={cx(
                      'px-0.75 border-0 rounded-xs bg-transparent cursor-pointer nth-[4n+1]:ml-1.5 hover:bg-hover',
                      changed.has(o + i) ? 'text-chg font-semibold' : 'text-fg2',
                    )}
                    aria-label={`${hex32(mem.addr + o + i)} = ${hex8(b)}`}
                    onClick={() => void edit(o + i)}
                  >
                    {hex8(b)}
                  </button>
                ))}
              </span>
              <span className="text-muted" aria-hidden="true">
                {mem.bytes
                  .slice(o, o + ROW)
                  .map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '.'))
                  .join('')}
              </span>
            </div>
          ))}
        </Out>
      )}
    </div>
  );
}
