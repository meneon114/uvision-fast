import { Check } from '../ui/Check';
import { useEffect, useRef, useState } from 'react';
import { uartSend } from '../../state/ide';
import { useIde } from '../../state/store';
import { CMD_WIN, PromptForm, PromptInput, UartOut } from './Out';

export function UartWindow() {
  const text = useIde((s) => s.dbg?.uart1_output ?? s.periph?.uart1_text ?? '');
  const [draft, setDraft] = useState('');
  const [cr, setCr] = useState(true);
  const out = useRef<HTMLPreElement>(null);
  useEffect(() => {
    out.current?.scrollTo({ top: out.current.scrollHeight });
  }, [text]);
  return (
    <div className={CMD_WIN}>
      <UartOut ref={out} role="log" aria-label="UART #1 output">
        {text || <span className="text-muted">USART1 output (TX) appears here.</span>}
      </UartOut>
      <PromptForm
        prompt="RX"
        onSubmit={() => {
          if (!draft && !cr) return;
          void uartSend(draft + (cr ? '\r' : ''));
          setDraft('');
        }}
      >
        <PromptInput aria-label="Send to USART1 RX" placeholder="Type and press Enter to send to USART1 RX" value={draft} onValueChange={setDraft} />
        <Check checked={cr} onChange={setCr}>
          Append CR
        </Check>
      </PromptForm>
    </div>
  );
}
