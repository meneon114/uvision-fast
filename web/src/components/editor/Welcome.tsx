import { Button } from '@ui-sdk/react/button';
import { exec } from '../../lib/commands';
import { baseName } from '../../lib/format';
import { openPath, firstSource } from '../../state/ide';
import { openDialog, useIde } from '../../state/store';
import { Icon } from '../Icon';
import { setSearch } from '../../lib/nav';

export function Welcome() {
  const recent = useIde((s) => s.recent);
  const project = useIde((s) => s.project);
  const hostKind = useIde((s) => s.hostKind);
  const booted = useIde((s) => s.booted);
  const src = firstSource(project);

  return (
    <div className="welcome">
      <div className="welcome-inner">
        <h1>
          <Icon name="chip" /> µVision Fast
        </h1>
        <p className="muted">Keil µVision 5 style assembler, simulator and debugger for the STM32F103 (Blue Pill).</p>
        {project && src ? (
          <div className="welcome-block">
            <h2>{project.project_file ? baseName(project.project_file) : 'Current file'}</h2>
            <Button data-variant="solid" onClick={() => setSearch({ file: src })}>
              <Icon name="go-to-file" /> Open {baseName(src)}
            </Button>
          </div>
        ) : null}
        <div className="welcome-cols">
          <div className="welcome-block">
            <h2>Start</h2>
            <ul className="linklist">
              <li>
                <button type="button" onClick={() => openDialog('examples')}>
                  <Icon name="beaker" /> Open a lab example (GPIO, SysTick, EXTI, TIM2)
                </button>
              </li>
              <li>
                <button type="button" onClick={() => openDialog('newProject')}>
                  <Icon name="new-folder" /> New µVision project
                </button>
              </li>
              <li>
                <button type="button" onClick={() => exec('file.open')}>
                  <Icon name="folder-opened" /> {hostKind === 'web' ? 'Open from workspace' : 'Open project or file'}
                </button>
              </li>
              <li>
                <button type="button" onClick={() => openDialog('shortcuts')}>
                  <Icon name="keyboard" /> Keyboard shortcuts
                </button>
              </li>
            </ul>
          </div>
          <div className="welcome-block">
            <h2>Recent</h2>
            {recent.length ? (
              <ul className="linklist">
                {recent.map((p) => (
                  <li key={p}>
                    <button type="button" onClick={() => void openPath(p)} title={p}>
                      <Icon name="project" /> {baseName(p)} <span className="muted">{p}</span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="muted">{booted ? 'No recent projects yet.' : 'Loading workspace...'}</p>
            )}
          </div>
        </div>
        {hostKind === 'web' ? (
          <p className="muted small">Files live in this browser (IndexedDB). Drop .s, .c or .uvprojx files anywhere to import them.</p>
        ) : null}
      </div>
    </div>
  );
}
