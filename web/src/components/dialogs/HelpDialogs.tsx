import { Button } from '../ui/Button';
import { formatShortcut } from '@ui-sdk/react/shortcuts';
import { COMMANDS } from '../../lib/commands';
import { useIde } from '../../state/store';
import { CAPS, Kv } from '../ui/kit';
import { closeDialog, Modal } from './Modal';

export function ShortcutsDialog() {
  const withKeys = COMMANDS.filter((c) => c.keys);
  const cats = [...new Set(withKeys.map((c) => c.category))];
  return (
    <Modal name="shortcuts" title="Keyboard Shortcuts" size="md" footer={<Button onClick={closeDialog}>Close</Button>}>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-x-6 gap-y-4">
        {cats.map((cat) => (
          <section key={cat}>
            <h3 className={`${CAPS} mb-1.5`}>{cat}</h3>
            <Kv>
              {withKeys
                .filter((c) => c.category === cat)
                .map((c) => (
                  <div key={c.id}>
                    <dt>{c.title}</dt>
                    <dd>
                      <kbd>{formatShortcut(c.keys!)}</kbd>
                    </dd>
                  </div>
                ))}
            </Kv>
          </section>
        ))}
      </div>
    </Modal>
  );
}

export function AboutDialog() {
  const hostKind = useIde((s) => s.hostKind);
  return (
    <Modal
      name="about"
      title="About uVision Fast"
      size="sm"
      footer={
        <Button variant="solid" onClick={closeDialog}>
          OK
        </Button>
      }
    >
      <p>
        A Keil µVision 5 style IDE for STM32F103 (Blue Pill) assembly: build with the built-in armasm-compatible assembler, debug on the Cortex-M3 simulator,
        and flash the board.
      </p>
      <Kv>
        <div>
          <dt>Host</dt>
          <dd>{hostKind === 'web' ? 'Browser (offline PWA, WebAssembly core)' : 'VS Code extension (uvcli)'}</dd>
        </div>
        <div>
          <dt>Device</dt>
          <dd>STM32F103C8, Cortex-M3, 64 KB flash, 20 KB SRAM</dd>
        </div>
        <div>
          <dt>Course</dt>
          <dd>RUET CSE 3106, Labs 3 to 6</dd>
        </div>
      </Kv>
    </Modal>
  );
}
