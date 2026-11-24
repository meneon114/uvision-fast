import { Input as BaseInput } from '@ui-sdk/react/input';
import type { ComponentProps } from 'react';
import { cx } from '../../lib/cx';

type Props = Omit<ComponentProps<typeof BaseInput>, 'className'> & {
  className?: string;
  mono?: boolean;
  // 22px tall, for inline rows in panels.
  compact?: boolean;
  // No box until focused, for the command lines.
  bare?: boolean;
};

export function Input({ mono, compact, bare, className, ...props }: Props) {
  return (
    <BaseInput
      {...props}
      className={cx(
        'min-w-0 border rounded-sm text-fg focus:outline-1 focus:outline-acc focus:border-acc',
        compact ? 'h-5.5' : 'h-6.5',
        mono ? 'font-mono text-sm' : compact ? 'text-sm' : 'text-ui',
        bare ? 'px-1 border-transparent bg-transparent' : 'px-2 border-border2 bg-bg',
        className,
      )}
    />
  );
}
