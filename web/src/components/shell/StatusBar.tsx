import { showPanel } from '../../state/ide';
import { useIde } from '../../state/store';
import { cx } from '../../lib/cx';
import { Icon } from '../Icon';

// One status bar item. Icons in it are 14px.
const SI = 'inline-flex items-center gap-1 px-2 h-full flex-none [&_.codicon]:text-md';
const MONO = 'font-mono text-xs';

export function StatusBar() {
  const session = useIde((s) => s.session);
  const dbg = useIde((s) => s.dbg);
  const build = useIde((s) => s.build);
  const building = useIde((s) => s.building);
  const status = useIde((s) => s.status);
  const cursor = useIde((s) => s.cursor);
  const device = useIde((s) => s.project?.options.device);
  const pwa = useIde((s) => s.pwa);
  const hostKind = useIde((s) => s.hostKind);
  const debugging = session === 'on';

  return (
    <footer
      className="h-6 flex items-center gap-0.5 px-1.5 bg-status text-statusfg border-t border-border text-sm flex-none overflow-hidden whitespace-nowrap data-debug:bg-dbgstatus data-debug:text-ondbg data-debug:border-dbgstatus"
      data-debug={debugging || undefined}
      aria-label="Status bar"
    >
      {debugging && dbg ? (
        <>
          <span className={SI}>
            <Icon name="debug-alt" /> {dbg.backend === 'hardware' ? 'ST-Link Debugger' : 'Simulation'}
          </span>
          <span className={cx(SI, MONO)}>t1: {dbg.registers.time_sec.toFixed(8)} sec</span>
          <span className={cx(SI, MONO, 'compact:hidden')}>States: {dbg.registers.cycles}</span>
        </>
      ) : (
        <button
          type="button"
          className={cx(SI, 'border-0 bg-transparent cursor-pointer hover:bg-current/12')}
          onClick={() => showPanel('build')}
          aria-label="Show build output"
        >
          {building ? (
            <>
              <Icon name="loading" className="animate-spin" /> Building
            </>
          ) : build ? (
            <>
              <Icon name={build.success ? 'check' : 'error'} /> {build.success ? 'Build OK' : 'Build failed'}
              <span className="inline-flex items-center gap-0.75 ml-1.5">
                <Icon name="error" /> {build.errors} <Icon name="warning" /> {build.warnings}
              </span>
            </>
          ) : (
            <>
              <Icon name="circle-large-outline" /> Not built
            </>
          )}
        </button>
      )}
      <span className="block flex-[0_1_auto] min-w-0 px-2 overflow-hidden text-ellipsis leading-6" role="status" aria-live="polite">
        {status}
      </span>
      <span className="flex-1" />
      <span className={SI}>
        L:{cursor.line} C:{cursor.col}
      </span>
      <span className={cx(SI, 'compact:hidden')}>{device || 'STM32F103C8'}</span>
      <span className={cx(SI, 'compact:hidden')}>Thumb-2</span>
      {hostKind === 'web' ? (
        <span className={cx(SI, 'compact:hidden')} title={pwa.offlineReady ? 'All files cached for offline use' : 'Caching for offline use'}>
          <Icon name={pwa.offlineReady ? 'pass' : 'cloud'} /> {pwa.offlineReady ? 'Offline ready' : 'Online'}
        </span>
      ) : null}
    </footer>
  );
}
