import type { SideView } from '../../lib/search';
import { DebugView } from './DebugView';
import { FindView } from './FindView';
import { FlashView } from './FlashView';
import { PeripheralsView } from './PeripheralsView';
import { ProjectView } from './ProjectView';

const TITLES: Record<SideView, string> = {
  project: 'Project',
  find: 'Find in Files',
  debug: 'Debug',
  peripherals: 'Peripherals',
  flash: 'Flash',
};

export function SideBar({ view }: { view: SideView }) {
  return (
    <aside className="side" aria-label={`${TITLES[view]} view`}>
      {view === 'project' ? <ProjectView title={TITLES.project} /> : null}
      {view === 'find' ? <FindView title={TITLES.find} /> : null}
      {view === 'debug' ? <DebugView title={TITLES.debug} /> : null}
      {view === 'peripherals' ? <PeripheralsView title={TITLES.peripherals} /> : null}
      {view === 'flash' ? <FlashView title={TITLES.flash} /> : null}
    </aside>
  );
}
