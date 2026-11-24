import { Checkbox, CheckboxIndicator } from '@ui-sdk/react/checkbox';
import { Dialog, DialogBackdrop, DialogClose, DialogContent, DialogDescription, DialogTitle } from '@ui-sdk/react/dialog';
import { Field, FieldDescription, FieldLabel } from '@ui-sdk/react/field';
import { Input } from '@ui-sdk/react/input';
import { Select, SelectContent, SelectItem, SelectItemIndicator, SelectTrigger, SelectValue } from '@ui-sdk/react/select';
import type { ReactNode } from 'react';
import { ide, useIde, type DialogName } from '../../state/store';
import { Icon } from '../Icon';

export function closeDialog(): void {
  ide.set({ dialog: null });
}

export function Modal({
  name,
  title,
  description,
  size = 'md',
  children,
  footer,
  onClose,
}: {
  name: DialogName;
  title: string;
  description?: ReactNode;
  size?: 'sm' | 'md' | 'lg';
  children?: ReactNode;
  footer?: ReactNode;
  onClose?: () => void;
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
      <DialogBackdrop className="dlg-backdrop" />
      <DialogContent className={`dlg dlg-${size}`}>
        <header className="dlg-head">
          <DialogTitle>{title}</DialogTitle>
          <DialogClose className="icon-btn" aria-label="Close">
            <Icon name="close" />
          </DialogClose>
        </header>
        {description ? <DialogDescription className="dlg-desc">{description}</DialogDescription> : null}
        {open ? <div className="dlg-body">{children}</div> : null}
        {footer ? <footer className="dlg-foot">{footer}</footer> : null}
      </DialogContent>
    </Dialog>
  );
}

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
    <Field className="field">
      <FieldLabel>{label}</FieldLabel>
      <Input value={value} onValueChange={onChange} className={mono ? 'mono' : undefined} placeholder={placeholder} autoFocus={autoFocus} />
      {hint ? <FieldDescription>{hint}</FieldDescription> : null}
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
    <Field className="field">
      <FieldLabel>{label}</FieldLabel>
      <Select value={value} onValueChange={(v) => v && onChange(v as T)}>
        <SelectTrigger className="select-trigger">
          <SelectValue />
        </SelectTrigger>
        <SelectContent className="select-list">
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
    <label className="check">
      <Checkbox
        checked={checked}
        onCheckedChange={(c) => {
          const v = c === true;
          onChange(v);
        }}
      >
        <CheckboxIndicator />
      </Checkbox>
      {label}
    </label>
  );
}
