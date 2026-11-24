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
import { closeDialog, Modal } from './Modal';

const CATEGORIES = [...new Set(COMMANDS.map((c) => c.category))];

export function PaletteDialog() {
  return (
    <Modal name="palette" title="Command Palette" size="md">
      <Command
        className="palette"
        onAction={(id) => {
          closeDialog();
          // Let the dialog return focus first, so commands that move focus win.
          requestAnimationFrame(() => exec(String(id)));
        }}
      >
        <div className="palette-input">
          <Icon name="search" />
          <CommandInput aria-label="Search commands" placeholder="Type a command: build, step, memory, theme..." autoFocus />
        </div>
        <CommandEmpty className="empty">No matching commands.</CommandEmpty>
        <CommandList className="palette-list">
          {CATEGORIES.map((cat) => (
            <CommandGroup key={cat}>
              <CommandGroupLabel>{cat}</CommandGroupLabel>
              {COMMANDS.filter((c) => c.category === cat).map((c) => (
                <CommandItem key={c.id} value={c.id} label={`${cat}: ${c.title}`} keywords={[cat, c.id]} disabled={!isEnabled(c.id)}>
                  <CommandItemIcon>
                    <Icon name={c.icon ?? 'symbol-event'} />
                  </CommandItemIcon>
                  <span>
                    <span className="muted">{cat}: </span>
                    {c.title}
                  </span>
                  {c.keys ? <CommandShortcut shortcut={c.keys} /> : null}
                </CommandItem>
              ))}
            </CommandGroup>
          ))}
        </CommandList>
      </Command>
    </Modal>
  );
}
