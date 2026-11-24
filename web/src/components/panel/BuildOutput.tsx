import { useEffect, useRef } from 'react';
import { setSearch } from '../../lib/nav';
import { useIde } from '../../state/store';

const DIAG = /^(.*?)\((\d+)\):\s*(error|warning|note)\b/i;

export function BuildOutput() {
  const log = useIde((s) => s.buildLog);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight });
  }, [log]);
  if (!log) return <div className="out muted">Build output appears here. Press F7 to build the target.</div>;
  return (
    <div className="out mono" ref={ref} role="log" aria-label="Build output">
      {log.split('\n').map((l, i) => {
        const m = DIAG.exec(l);
        if (m) {
          return (
            <button key={i} type="button" className={`ol diag ${m[3].toLowerCase()}`} onClick={() => setSearch({ file: m[1], line: Number(m[2]) })}>
              {l}
            </button>
          );
        }
        const ok = /- 0 Error\(s\)/.test(l);
        const bad = /[1-9]\d* Error\(s\)|failed/i.test(l);
        return (
          <div key={i} className={`ol${ok ? ' ok' : bad ? ' bad' : ''}`}>
            {l || ' '}
          </div>
        );
      })}
    </div>
  );
}
