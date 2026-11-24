import { Button } from '../ui/Button';
import { exec } from '../../lib/commands';
import { baseName } from '../../lib/format';
import { openPath, firstSource } from '../../state/ide';
import { openDialog, useIde } from '../../state/store';
import { Icon } from '../Icon';
import { setSearch } from '../../lib/nav';

const H2 = 'text-lg font-normal text-fg mt-5 mb-2';
const LINKS = 'list-none m-0 p-0 flex flex-col gap-0.5';
const LINK =
  'flex items-center gap-2 w-full py-0.75 px-1.5 border-0 rounded-sm bg-transparent text-acc text-left cursor-pointer overflow-hidden whitespace-nowrap text-ellipsis hover:bg-hover active:translate-y-px';

export function Welcome() {
  const recent = useIde((s) => s.recent);
  const project = useIde((s) => s.project);
  const hostKind = useIde((s) => s.hostKind);
  const booted = useIde((s) => s.booted);
  const src = firstSource(project);

  return (
    <div className="flex-1 overflow-auto flex justify-center py-12 px-6 bg-bg phone:py-6 phone:px-4">
      <div className="w-full max-w-190">
        <h1 className="text-4xl font-normal text-fg mb-1">
          <Icon name="chip" /> µVision Fast
        </h1>
        <p className="my-[1em] text-muted">Keil µVision 5 style assembler, simulator and debugger for the STM32F103 (Blue Pill).</p>
        {project && src ? (
          <div>
            <h2 className={H2}>{project.project_file ? baseName(project.project_file) : 'Current file'}</h2>
            <Button variant="solid" onClick={() => setSearch({ file: src })}>
              <Icon name="go-to-file" /> Open {baseName(src)}
            </Button>
          </div>
        ) : null}
        <div className="grid grid-cols-2 gap-8 my-7 phone:grid-cols-1">
          <div>
            <h2 className={H2}>Start</h2>
            <ul className={LINKS}>
              <li>
                <button type="button" className={LINK} onClick={() => openDialog('examples')}>
                  <Icon name="beaker" /> Open a lab example (GPIO, SysTick, EXTI, TIM2)
                </button>
              </li>
              <li>
                <button type="button" className={LINK} onClick={() => openDialog('newProject')}>
                  <Icon name="new-folder" /> New µVision project
                </button>
              </li>
              <li>
                <button type="button" className={LINK} onClick={() => exec('file.open')}>
                  <Icon name="folder-opened" /> {hostKind === 'web' ? 'Open from workspace' : 'Open project or file'}
                </button>
              </li>
              <li>
                <button type="button" className={LINK} onClick={() => openDialog('shortcuts')}>
                  <Icon name="keyboard" /> Keyboard shortcuts
                </button>
              </li>
            </ul>
          </div>
          <div>
            <h2 className={H2}>Recent</h2>
            {recent.length ? (
              <ul className={LINKS}>
                {recent.map((p) => (
                  <li key={p}>
                    <button type="button" className={LINK} onClick={() => void openPath(p)} title={p}>
                      <Icon name="project" /> {baseName(p)} <span className="text-muted overflow-hidden text-ellipsis text-sm">{p}</span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="my-[1em] text-muted">{booted ? 'No recent projects yet.' : 'Loading workspace...'}</p>
            )}
          </div>
        </div>
        {hostKind === 'web' ? (
          <p className="my-[1em] text-muted text-xs">Files live in this browser (IndexedDB). Drop .s, .c or .uvprojx files anywhere to import them.</p>
        ) : null}
      </div>
    </div>
  );
}
