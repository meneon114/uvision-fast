import { ToolbarButton } from '@ui-sdk/react/toolbar';
import { Tooltip, TooltipContent, TooltipTrigger } from '@ui-sdk/react/tooltip';
import { formatShortcut } from '@ui-sdk/react/shortcuts';
import { createContext, useContext } from 'react';
import { cmd, isEnabled } from '../../lib/commands';
import { cx } from '../../lib/cx';
import { Icon } from '../Icon';

type Size = 'md' | 'sm' | 'activity';

// Title bars and side views use the 24px buttons; the Keil toolbar the 28px ones.
const SizeContext = createContext<Size>('md');
export const TbSize = SizeContext.Provider;

const BASE =
  'inline-flex items-center justify-center gap-1.25 border-0 rounded-sm bg-transparent text-sm cursor-pointer flex-none transition-[background-color,translate] duration-100 ease-snap disabled:opacity-40 disabled:cursor-default data-disabled:opacity-40 data-disabled:cursor-default enabled:not-data-disabled:active:translate-y-px dark:aria-pressed:text-accfg';

const PRESSABLE =
  'enabled:not-data-disabled:hover:bg-hover enabled:not-data-disabled:active:bg-press aria-pressed:bg-sel aria-pressed:inset-ring aria-pressed:inset-ring-acc/35';

const SIZE: Record<Size, string> = {
  md: `h-7 min-w-7 text-fg2 aria-pressed:text-acc ${PRESSABLE}`,
  sm: `h-6 min-w-6 text-muted ${PRESSABLE}`,
  activity:
    'relative w-12 h-12 rounded-none text-muted enabled:hover:text-fg aria-pressed:text-fg aria-pressed:before:absolute aria-pressed:before:left-0 aria-pressed:before:inset-y-2.5 aria-pressed:before:w-0.5 aria-pressed:before:bg-acc phone:w-10 phone:h-11',
};

const ICON: Record<Size, string | undefined> = { md: undefined, sm: undefined, activity: 'text-3xl phone:text-2xl' };

interface Props {
  label: string;
  icon: string;
  keys?: string;
  text?: string;
  pressed?: boolean;
  disabled?: boolean;
  tone?: 'go' | 'stop';
  onClick: () => void;
}

// A toolbar button with a tooltip naming it and its shortcut.
export function TbButton({ label, icon, keys, text, pressed, disabled, tone, onClick }: Props) {
  const size = useContext(SizeContext);
  return (
    <Tooltip delay={450} type="label">
      <TooltipTrigger
        render={
          <ToolbarButton
            aria-label={label}
            aria-pressed={pressed}
            disabled={disabled}
            className={cx(BASE, SIZE[size], text ? 'pl-1.5 pr-2' : 'px-1.5')}
            onClick={onClick}
          />
        }
      >
        <Icon name={icon} className={tone === 'go' ? 'text-go' : tone === 'stop' ? 'text-stop' : ICON[size]} />
        {text ? <span className="compact:hidden">{text}</span> : null}
      </TooltipTrigger>
      <TooltipContent side="bottom" className="flex items-center gap-2 px-2 py-1 border border-border2 rounded-sm bg-bg text-fg shadow-pop text-sm">
        {label}
        {keys ? <kbd>{formatShortcut(keys)}</kbd> : null}
      </TooltipContent>
    </Tooltip>
  );
}

// The same, bound to a registered command.
export function CmdButton({ id, text, pressed, tone, label }: { id: string; text?: string; pressed?: boolean; tone?: 'go' | 'stop'; label?: string }) {
  const c = cmd(id);
  return (
    <TbButton
      label={label ?? c.title.replace(/\.\.\.$/, '')}
      icon={c.icon ?? 'circle'}
      keys={c.keys}
      text={text}
      pressed={pressed}
      tone={tone}
      disabled={!isEnabled(id)}
      onClick={() => void c.run()}
    />
  );
}
