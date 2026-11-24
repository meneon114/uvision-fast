import { Button } from '@ui-sdk/react/button';
import { exec } from '../../lib/commands';
import { useIde } from '../../state/store';
import { Icon } from '../Icon';
import { Section } from './Section';
import { ViewTitle } from './ViewTitle';
import { CmdButton } from '../shell/TbButton';

export function FlashView({ title }: { title: string }) {
  const o = useIde((s) => s.project?.options);
  const hostKind = useIde((s) => s.hostKind);
  const build = useIde((s) => s.build);
  const log = useIde((s) => s.flashLog);
  const webSerial = typeof navigator !== 'undefined' && 'serial' in navigator;
  return (
    <div className="view">
      <ViewTitle title={title}>
        <CmdButton id="flash.download" />
        <CmdButton id="flash.export" />
      </ViewTitle>
      <div className="view-scroll">
        <Section title="Target">
          <dl className="kv">
            <dt>Device</dt>
            <dd>{o?.device ?? 'STM32F103C8'}</dd>
            <dt>Flash (IROM1)</dt>
            <dd className="mono">{o ? `0x${o.ro_base.toString(16).toUpperCase()} size 0x${o.ro_size.toString(16).toUpperCase()}` : '-'}</dd>
            <dt>Last build</dt>
            <dd>{build ? (build.success ? `OK, ${build.elapsed_ms} ms` : `${build.errors} error(s)`) : 'not built'}</dd>
          </dl>
        </Section>
        <Section title="Programmer">
          {hostKind === 'web' ? (
            <>
              <p className="small">
                {webSerial
                  ? 'Flashes over a USB-serial adapter with the STM32 ROM bootloader (BOOT0 = 1, then reset).'
                  : 'This browser has no Web Serial. Use Chrome or Edge, or export the .hex/.bin and flash it with STM32CubeProgrammer.'}
              </p>
              <div className="row">
                <Button data-variant="solid" onClick={() => exec('flash.download')}>
                  <Icon name="desktop-download" /> Download (F8)
                </Button>
                <Button onClick={() => exec('flash.export')}>
                  <Icon name="export" /> Export
                </Button>
              </div>
            </>
          ) : (
            <dl className="kv">
              <dt>Interface</dt>
              <dd>{o?.hw_interface ?? 'stlink'}</dd>
              <dt>Target config</dt>
              <dd>{o?.hw_target ?? 'stm32f1x'}</dd>
              <dt>Tool</dt>
              <dd>OpenOCD (bundled)</dd>
            </dl>
          )}
        </Section>
        {log.lines.length ? (
          <Section title="Log">
            <pre className="mono small flash-log">{log.lines.join('\n')}</pre>
          </Section>
        ) : null}
      </div>
    </div>
  );
}
