import { showPanel } from '../../state/ide';
import { useIde } from '../../state/store';
import { Icon } from '../Icon';

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
    <footer className="status" data-debug={debugging || undefined} aria-label="Status bar">
      {debugging && dbg ? (
        <>
          <span className="si">
            <Icon name="debug-alt" /> {dbg.backend === 'hardware' ? 'ST-Link Debugger' : 'Simulation'}
          </span>
          <span className="si mono">t1: {dbg.registers.time_sec.toFixed(8)} sec</span>
          <span className="si mono hide-sm">States: {dbg.registers.cycles}</span>
        </>
      ) : (
        <button type="button" className="si si-btn" onClick={() => showPanel('build')} aria-label="Show build output">
          {building ? (
            <>
              <Icon name="loading" className="spin" /> Building
            </>
          ) : build ? (
            <>
              <Icon name={build.success ? 'check' : 'error'} /> {build.success ? 'Build OK' : 'Build failed'}
              <span className="si-sub">
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
      <span className="si si-msg" role="status" aria-live="polite">
        {status}
      </span>
      <span className="grow" />
      <span className="si">
        L:{cursor.line} C:{cursor.col}
      </span>
      <span className="si hide-sm">{device || 'STM32F103C8'}</span>
      <span className="si hide-sm">Thumb-2</span>
      {hostKind === 'web' ? (
        <span className="si hide-sm" title={pwa.offlineReady ? 'All files cached for offline use' : 'Caching for offline use'}>
          <Icon name={pwa.offlineReady ? 'pass' : 'cloud'} /> {pwa.offlineReady ? 'Offline ready' : 'Online'}
        </span>
      ) : null}
    </footer>
  );
}
