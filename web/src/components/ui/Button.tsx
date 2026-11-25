import { Button as BaseButton } from '@ui-sdk/react/button';
import type { ComponentProps } from 'react';
import { cx } from '../../lib/cx';

// The one button style. `solid` is the primary action.
export const BUTTON =
  'inline-flex items-center gap-1.5 h-7 px-3 border rounded-sm text-ui cursor-pointer transition-[background-color,translate] duration-120 ease-snap data-disabled:opacity-50 data-disabled:cursor-default not-data-disabled:active:translate-y-px';

const VARIANT = {
  default: 'border-border2 bg-bg text-fg not-data-disabled:hover:bg-hover not-data-disabled:active:bg-press',
  solid: 'border-acc bg-acc text-accfg not-data-disabled:hover:bg-acc-hover',
};

type Props = Omit<ComponentProps<typeof BaseButton>, 'className'> & { className?: string; variant?: keyof typeof VARIANT };

export function Button({ variant = 'default', className, ...props }: Props) {
  return <BaseButton {...props} className={cx(BUTTON, VARIANT[variant], className)} />;
}

// A bare icon button for close and remove actions.
export function iconButton(size: 'md' | 'sm' = 'md'): string {
  return cx(
    'inline-grid place-items-center p-0 border-0 rounded-sm bg-transparent text-muted cursor-pointer hover:bg-hover hover:text-fg active:bg-press active:translate-y-px',
    size === 'md' ? 'size-5.5' : 'size-4.5',
  );
}
