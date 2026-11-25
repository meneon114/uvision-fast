import type { ComponentProps, ReactNode } from 'react';
import { cx } from '../../lib/cx';

// Small text building blocks that more than one view uses.

// Content inside a side bar section lines up with the section title.
export const INSET = 'ml-5 mr-3';

// A small uppercase heading over a group of rows.
export const CAPS = 'text-2xs font-semibold tracking-[0.03em] uppercase text-muted';

export const ROW = 'flex items-center gap-2 flex-wrap';

export function Row({ className, ...props }: ComponentProps<'div'>) {
  return <div {...props} className={cx(ROW, className)} />;
}

// "Nothing here" text in a view or panel.
export const EMPTY = 'm-0 py-1.5 px-5 text-muted text-sm';

export function Empty({ className, children }: { className?: string; children: ReactNode }) {
  return <p className={cx(EMPTY, className)}>{children}</p>;
}

export function Note({ warn, className, children }: { warn?: boolean; className?: string; children: ReactNode }) {
  return <p className={cx('mt-2 mb-0 py-2 px-2.5 border-l-2 bg-chrome2 text-fg2 text-sm', warn ? 'border-warn' : 'border-acc', className)}>{children}</p>;
}

// Key and value pairs: <dt>/<dd> stacked, or wrapped in <div>s for two columns.
export function Kv({ className, ...props }: ComponentProps<'dl'>) {
  return (
    <dl
      {...props}
      className={cx(
        'm-0 flex flex-col gap-0.75 text-sm [&>div]:grid [&>div]:grid-cols-[minmax(80px,38%)_1fr] [&>div]:gap-2 [&_dt]:text-muted [&_dd]:m-0 [&_dd]:text-fg [&_dd]:wrap-anywhere',
        className,
      )}
    />
  );
}

// A side bar view: title on top, the rest scrolls.
export const VIEW = 'flex-1 flex flex-col min-h-0';
export const VIEW_SCROLL = 'flex-1 overflow-auto min-h-0';
