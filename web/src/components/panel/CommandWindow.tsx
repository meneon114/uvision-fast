import { useEffect, useRef, useState } from 'react';
import { runCommand } from '../../state/ide';
import { useIde } from '../../state/store';
import { CMD_WIN, Line, Out, PromptForm, PromptInput } from './Out';

export function CommandWindow() {
  const log = useIde((s) => s.cmdLog);
  const history = useIde((s) => s.cmdHistory);
  const [text, setText] = useState('');
  const [pos, setPos] = useState(-1);
  const out = useRef<HTMLDivElement>(null);
  useEffect(() => {
    out.current?.scrollTo({ top: out.current.scrollHeight });
  }, [log]);
  return (
    <div className={CMD_WIN}>
      <Out mono ref={out} role="log" aria-label="Command output">
        {log.length === 0 ? <Line tone="muted">Type help for Keil-style debugger commands (g, t, p, bs, bc, d, e, r, reset).</Line> : null}
        {log.map((l, i) => (
          <Line key={i} tone={l.kind}>
            {l.kind === 'in' ? `> ${l.text}` : l.text}
          </Line>
        ))}
      </Out>
      <PromptForm
        prompt=">"
        onSubmit={() => {
          void runCommand(text);
          setText('');
          setPos(-1);
        }}
      >
        <PromptInput
          aria-label="Debugger command"
          placeholder="ASSIGN  BreakDisable  BreakEnable  BreakKill  BreakList  BreakSet  g  t  p  d  e  r"
          value={text}
          onValueChange={setText}
          onKeyDown={(e: React.KeyboardEvent<HTMLInputElement>) => {
            if (e.key === 'ArrowUp' && history.length) {
              e.preventDefault();
              const p = pos < 0 ? history.length - 1 : Math.max(0, pos - 1);
              setPos(p);
              setText(history[p]);
            } else if (e.key === 'ArrowDown' && pos >= 0) {
              e.preventDefault();
              const p = pos + 1;
              if (p >= history.length) {
                setPos(-1);
                setText('');
              } else {
                setPos(p);
                setText(history[p]);
              }
            }
          }}
        />
      </PromptForm>
    </div>
  );
}
