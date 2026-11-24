import { useShortcut } from '@ui-sdk/react/shortcuts';
import { COMMANDS, type Command } from '../../lib/commands';

const BOUND = COMMANDS.filter((c): c is Command & { keys: string } => !!c.keys);

function Bind({ c }: { c: Command & { keys: string } }) {
  // Always bound, so a disabled F5 never falls through to a browser reload.
  useShortcut(
    c.keys,
    () => {
      if (!c.enabled || c.enabled()) void c.run();
    },
    { description: c.title, allowInInput: true },
  );
  return null;
}

// Keil's function keys work everywhere, the editor included.
export function GlobalShortcuts() {
  return (
    <>
      {BOUND.map((c) => (
        <Bind key={c.id} c={c} />
      ))}
    </>
  );
}
