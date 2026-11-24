import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandGroupLabel,
  CommandInput,
  CommandItem,
  CommandItemIcon,
  CommandList,
  CommandShortcut,
} from '@ui-sdk/react/command';
import { COMMANDS, exec, isEnabled } from '../../lib/commands';
import { Icon } from '../Icon';
import { EMPTY } from '../ui/kit';
import { closeDialog, Modal } from './Modal';

const ITEM =
  'h-6.5 flex items-center gap-2 px-2.5 rounded-sm text-fg2 cursor-pointer data-highlighted:bg-sel data-highlighted:text-fg data-disabled:opacity-45 [&>span]:flex-1 [&>span]:overflow-hidden [&>span]:whitespace-nowrap [&>span]:text-ellipsis [&_.codicon]:text-lg [&_.codicon]:text-muted data-highlighted:[&_.codicon]:text-acc';

const CATEGORIES = [...new Set(COMMANDS.map((c) => c.category))];

export function PaletteDialog() {
  return (
    <Modal name="palette" title="Command Palette" size="md">
      <Command
        className="flex flex-col min-h-0"
        onAction={(id) => {
          closeDialog();
          // Let the dialog return focus first, so commands that move focus win.
          requestAnimationFrame(() => exec(String(id)));
        }}
      >
        <div className="flex items-center gap-1.5 h-7.5 px-2 mb-1.5 border border-acc rounded-sm text-muted">
          <Icon name="search" />
          <CommandInput
            className="flex-1 h-full border-0 outline-none bg-transparent text-fg text-base"
            aria-label="Search commands"
            placeholder="Type a command: build, step, memory, theme..."
            autoFocus
          />
        </div>
        <CommandEmpty className={EMPTY}>No matching commands.</CommandEmpty>
        <CommandList className="max-h-[min(420px,60dvh)] overflow-auto">
          {CATEGORIES.map((cat) => (
            <CommandGroup key={cat}>
              <CommandGroupLabel className="pt-1.5 px-2.5 pb-0.5 text-2xs text-muted">{cat}</CommandGroupLabel>
              {COMMANDS.filter((c) => c.category === cat).map((c) => (
                <CommandItem className={ITEM} key={c.id} value={c.id} label={`${cat}: ${c.title}`} keywords={[cat, c.id]} disabled={!isEnabled(c.id)}>
                  <CommandItemIcon>
                    <Icon name={c.icon ?? 'symbol-event'} />
                  </CommandItemIcon>
                  <span>
                    <span className="text-muted">{cat}: </span>
                    {c.title}
                  </span>
                  {c.keys ? <CommandShortcut className="font-ui text-2xs leading-[normal] text-muted" shortcut={c.keys} /> : null}
                </CommandItem>
              ))}
            </CommandGroup>
          ))}
        </CommandList>
      </Command>
    </Modal>
  );
}
