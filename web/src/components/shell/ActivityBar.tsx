import { Toolbar } from '@ui-sdk/react/toolbar';
import { showView } from '../../lib/commands';
import type { SideView } from '../../lib/search';
import { useProjectSearch } from '../../lib/useProjectSearch';
import { ide, openDialog, useIde } from '../../state/store';
import { TbButton, TbSize } from './TbButton';

const VIEWS: { id: SideView; label: string; icon: string; keys?: string }[] = [
  { id: 'project', label: 'Project', icon: 'files' },
  { id: 'find', label: 'Find in Files', icon: 'search', keys: 'mod+shift+f' },
  { id: 'debug', label: 'Debug: Registers, Watch, Call Stack', icon: 'debug-alt' },
  { id: 'peripherals', label: 'Peripherals and Lab Board', icon: 'circuit-board' },
  { id: 'flash', label: 'Flash', icon: 'desktop-download' },
];

export function ActivityBar() {
  const search = useProjectSearch();
  const sideOpen = useIde((s) => s.sideOpen);
  const session = useIde((s) => s.session);
  const current = search.view ?? (session === 'off' ? 'project' : 'debug');

  return (
    <TbSize value="activity">
      <Toolbar
        aria-label="Views"
        orientation="vertical"
        className="w-12 flex-none flex flex-col items-center gap-0 pt-0 pb-1 px-0 border-0 border-r border-border rounded-none bg-chrome phone:w-10"
      >
        {VIEWS.map((v) => (
          <TbButton
            key={v.id}
            label={v.label}
            icon={v.icon}
            keys={v.keys}
            pressed={sideOpen && current === v.id}
            onClick={() => {
              if (sideOpen && current === v.id) ide.set({ sideOpen: false });
              else showView(v.id);
            }}
          />
        ))}
        <span className="flex-1" />
        <TbButton label="Options for Target" icon="settings-gear" keys="alt+f7" onClick={() => openDialog('options')} />
      </Toolbar>
    </TbSize>
  );
}
