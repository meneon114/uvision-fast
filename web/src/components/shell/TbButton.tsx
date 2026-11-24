import { ToolbarButton } from '@ui-sdk/react/toolbar';
import { Tooltip, TooltipContent, TooltipTrigger } from '@ui-sdk/react/tooltip';
import { formatShortcut } from '@ui-sdk/react/shortcuts';
import { cmd, isEnabled } from '../../lib/commands';
import { Icon } from '../Icon';

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
  return (
    <Tooltip delay={450} type="label">
      <TooltipTrigger
        render={
          <ToolbarButton
            aria-label={label}
            aria-pressed={pressed}
            disabled={disabled}
            data-tone={tone}
            className={text ? 'tb tb-text' : 'tb'}
            onClick={onClick}
          />
        }
      >
        <Icon name={icon} />
        {text ? <span>{text}</span> : null}
      </TooltipTrigger>
      <TooltipContent side="bottom" className="tip">
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
