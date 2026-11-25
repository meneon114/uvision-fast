import { Checkbox as BaseCheckbox, CheckboxIndicator } from '@ui-sdk/react/checkbox';
import type { ReactNode } from 'react';
import { cx } from '../../lib/cx';

export const CHECK_LABEL = 'inline-flex items-center gap-1.5 cursor-pointer';

// A labelled checkbox.
export function Check({
  checked,
  onChange,
  children,
  className,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  children: ReactNode;
  className?: string;
}) {
  return (
    <label className={cx(CHECK_LABEL, className ?? 'text-sm text-fg2')}>
      <BaseCheckbox
        checked={checked}
        onCheckedChange={(c) => onChange(c === true)}
        className="rounded-xs data-checked:bg-acc data-checked:border-acc data-checked:text-accfg"
      >
        <CheckboxIndicator />
      </BaseCheckbox>
      {children}
    </label>
  );
}
