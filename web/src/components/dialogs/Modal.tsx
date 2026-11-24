import { Dialog, DialogBackdrop, DialogClose, DialogContent, DialogDescription, DialogTitle } from '@ui-sdk/react/dialog';
import { Field, FieldDescription, FieldLabel } from '@ui-sdk/react/field';
import type { ReactNode } from 'react';
import { cx } from '../../lib/cx';
import { ide, useIde, type DialogName } from '../../state/store';
import { Icon } from '../Icon';
import { iconButton } from '../ui/Button';
import { Check } from '../ui/Check';
import { Input } from '../ui/Input';
import { Select, SelectContent, SelectItem, SelectItemIndicator, SelectTrigger, SelectValue } from '../ui/select';

export function closeDialog(): void {
  ide.set({ dialog: null });
}

const WIDTH = { sm: 'w-[min(420px,calc(100vw-32px))]', md: 'w-[min(560px,calc(100vw-32px))]', lg: 'w-[min(760px,calc(100vw-32px))]' };

export function Modal({
  name,
  title,
  description,
  size = 'md',
  children,
  footer,
  onClose,
  initialFocus,
}: {
  name: DialogName;
  title: string;
  description?: ReactNode;
  size?: 'sm' | 'md' | 'lg';
  children?: ReactNode;
  footer?: ReactNode;
  onClose?: () => void;
  /** Selector for the element focused on open. Without it ui-sdk focuses the first tabbable, the Close button. */
  initialFocus?: string;
}) {
  const open = useIde((s) => s.dialog === name);
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) {
          onClose?.();
          if (ide.get().dialog === name) closeDialog();
        }
      }}
    >
      <DialogBackdrop className="bg-overlay" />
      <DialogContent
        initialFocus={initialFocus}
        className={cx(
          'max-h-[calc(100dvh-64px)] flex flex-col gap-0 p-0 border border-border2 rounded-md bg-bg text-fg2 shadow-pop overflow-hidden',
          WIDTH[size],
        )}
      >
        <header className="flex items-center gap-2 h-10 pl-4 pr-2 border-b border-border flex-none">
          <DialogTitle className="flex-1 text-base font-semibold text-fg">{title}</DialogTitle>
          <DialogClose className={iconButton()} aria-label="Close">
            <Icon name="close" />
          </DialogClose>
        </header>
        {description ? <DialogDescription className="m-0 pt-2.5 px-4 text-muted text-sm">{description}</DialogDescription> : null}
        {open ? <div className="py-3.5 px-4 overflow-auto min-h-0 [&_p]:mt-0 [&_p]:mb-2.5">{children}</div> : null}
        {footer ? (
          <footer className="flex items-center justify-end flex-wrap gap-2 py-2.5 px-4 border-t border-border bg-chrome flex-none">{footer}</footer>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

// Two columns of fields, or one on phones and with `one`.
export function formGrid(one?: boolean): string {
  return cx('grid gap-x-4 gap-y-3 items-start', one ? 'grid-cols-1' : 'grid-cols-2 phone:grid-cols-1');
}

// Fields side by side across the whole grid, bottoms aligned.
export const FIELD_ROW = 'col-span-full flex items-end gap-2 *:data-[slot=field]:flex-1';

const FIELD = 'flex flex-col gap-1 min-w-0';
const LABEL = 'text-sm text-fg2';
const HINT = 'text-xs text-muted';

export function TextField({
  label,
  value,
  onChange,
  hint,
  mono,
  placeholder,
  autoFocus,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  hint?: string;
  mono?: boolean;
  placeholder?: string;
  autoFocus?: boolean;
}) {
  return (
    <Field className={FIELD}>
      <FieldLabel className={LABEL}>{label}</FieldLabel>
      <Input value={value} onValueChange={onChange} mono={mono} placeholder={placeholder} autoFocus={autoFocus} />
      {hint ? <FieldDescription className={HINT}>{hint}</FieldDescription> : null}
    </Field>
  );
}

export function SelectField<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
}) {
  return (
    <Field className={FIELD}>
      <FieldLabel className={LABEL}>{label}</FieldLabel>
      <Select value={value} onValueChange={(v) => v && onChange(v as T)}>
        <SelectTrigger className="h-6.5 flex items-center justify-between gap-2 px-2 border border-border2 rounded-sm bg-bg text-fg text-ui text-left cursor-pointer hover:border-muted">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map(([v, l]) => (
            <SelectItem key={v} value={v}>
              {l}
              <SelectItemIndicator />
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  );
}

export function CheckField({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <Check checked={checked} onChange={onChange}>
      {label}
    </Check>
  );
}
