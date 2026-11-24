import { Checkbox, CheckboxIndicator } from '@ui-sdk/react/checkbox';
import { Input } from '@ui-sdk/react/input';
import { useEffect, useRef, useState } from 'react';
import { uartSend } from '../../state/ide';
import { useIde } from '../../state/store';

export function UartWindow() {
  const text = useIde((s) => s.dbg?.uart1_output ?? s.periph?.uart1_text ?? '');
  const [draft, setDraft] = useState('');
  const [cr, setCr] = useState(true);
  const out = useRef<HTMLPreElement>(null);
  useEffect(() => {
    out.current?.scrollTo({ top: out.current.scrollHeight });
  }, [text]);
  return (
    <div className="cmdwin">
      <pre className="out mono uart" ref={out} role="log" aria-label="UART #1 output">
        {text || <span className="muted">USART1 output (TX) appears here.</span>}
      </pre>
      <form
        className="cmdin"
        onSubmit={(e) => {
          e.preventDefault();
          if (!draft && !cr) return;
          void uartSend(draft + (cr ? '\r' : ''));
          setDraft('');
        }}
      >
        <span aria-hidden="true">RX</span>
        <Input className="mono" aria-label="Send to USART1 RX" placeholder="Type and press Enter to send to USART1 RX" value={draft} onValueChange={setDraft} />
        <label className="check small">
          <Checkbox
            checked={cr}
            onCheckedChange={(c) => {
              const v = c === true;
              setCr(v);
            }}
          >
            <CheckboxIndicator />
          </Checkbox>
          Append CR
        </label>
      </form>
    </div>
  );
}
