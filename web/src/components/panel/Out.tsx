import type { ComponentProps, ReactNode } from 'react';
import { cx } from '../../lib/cx';
import { Input } from '../ui/Input';

// The scrolling text area every panel window uses.
const OUT = 'flex-1 m-0 pt-1 px-3 pb-1.5 overflow-auto bg-bg leading-[19px] min-h-0';

export function Out({ mono, muted, className, ...props }: ComponentProps<'div'> & { mono?: boolean; muted?: boolean }) {
  return <div {...props} className={cx(OUT, mono && 'font-mono text-ui', muted ? 'text-muted' : 'text-fg2', className)} />;
}

export function UartOut({ className, ...props }: ComponentProps<'pre'>) {
  return <pre {...props} className={cx(OUT, 'font-mono text-ui text-fg2 whitespace-pre-wrap', className)} />;
}

export type LineTone = 'in' | 'out' | 'err' | 'ok' | 'muted';

const TONE: Record<LineTone, string> = { in: 'text-acc', out: 'text-fg2', err: 'text-bp', ok: 'text-go font-semibold', muted: 'text-muted' };

export const LINE = 'whitespace-pre-wrap wrap-anywhere';

export function Line({ tone = 'out', children }: { tone?: LineTone; children: ReactNode }) {
  return <div className={cx(LINE, TONE[tone])}>{children}</div>;
}

// A window with output on top and a one-line prompt under it.
export const CMD_WIN = 'flex-1 flex flex-col min-h-0';

export function PromptForm({ prompt, onSubmit, children }: { prompt: string; onSubmit: () => void; children: ReactNode }) {
  return (
    <form
      className="h-7.5 flex items-center gap-1.5 px-3 border-t border-border bg-bg flex-none"
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit();
      }}
    >
      <span aria-hidden="true" className="text-acc font-mono text-sm">
        {prompt}
      </span>
      {children}
    </form>
  );
}

export function PromptInput(props: Omit<ComponentProps<typeof Input>, 'mono' | 'bare' | 'className'>) {
  return <Input {...props} mono bare className="flex-1" />;
}
