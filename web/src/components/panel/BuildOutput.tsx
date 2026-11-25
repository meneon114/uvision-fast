import { useEffect, useRef } from 'react';
import { setSearch } from '../../lib/nav';
import { useIde } from '../../state/store';
import { cx } from '../../lib/cx';
import { Line, LINE, Out } from './Out';

const DIAG = /^(.*?)\((\d+)\):\s*(error|warning|note)\b/i;

export function BuildOutput() {
  const log = useIde((s) => s.buildLog);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight });
  }, [log]);
  if (!log) return <Out muted>Build output appears here. Press F7 to build the target.</Out>;
  return (
    <Out mono ref={ref} role="log" aria-label="Build output">
      {log.split('\n').map((l, i) => {
        const m = DIAG.exec(l);
        if (m) {
          return (
            <button
              key={i}
              type="button"
              className={cx(
                LINE,
                'block w-full p-0 border-0 bg-transparent text-left cursor-pointer hover:bg-hover hover:underline',
                m[3].toLowerCase() === 'error' ? 'text-bp' : m[3].toLowerCase() === 'warning' ? 'text-warn' : 'text-fg2',
              )}
              onClick={() => setSearch({ file: m[1], line: Number(m[2]) })}
            >
              {l}
            </button>
          );
        }
        const ok = /- 0 Error\(s\)/.test(l);
        const bad = /[1-9]\d* Error\(s\)|failed/i.test(l);
        return (
          <Line key={i} tone={ok ? 'ok' : bad ? 'err' : 'out'}>
            {l || ' '}
          </Line>
        );
      })}
    </Out>
  );
}
