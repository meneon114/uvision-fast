import { Tabs, TabsList, TabsPanel, TabsTab } from '@ui-sdk/react/tabs';
import { Toolbar } from '@ui-sdk/react/toolbar';
import type { PanelId } from '../../lib/search';
import { setSearch } from '../../lib/nav';
import { useProjectSearch } from '../../lib/useProjectSearch';
import { refreshMemory } from '../../state/ide';
import { ide, useIde } from '../../state/store';
import { TbButton } from '../shell/TbButton';
import { BuildOutput } from './BuildOutput';
import { CommandWindow } from './CommandWindow';
import { FindResults } from './FindResults';
import { LogicAnalyzer } from './LogicAnalyzer';
import { MemoryWindow } from './MemoryWindow';
import { UartWindow } from './UartWindow';

const LABELS: Record<PanelId, string> = {
  build: 'Build Output',
  command: 'Command',
  find: 'Find in Files',
  mem1: 'Memory 1',
  mem2: 'Memory 2',
  mem3: 'Memory 3',
  mem4: 'Memory 4',
  uart: 'UART #1',
  logic: 'Logic Analyzer',
};

const EDIT_TABS: PanelId[] = ['build', 'command', 'find'];
const DEBUG_TABS: PanelId[] = ['command', 'build', 'mem1', 'mem2', 'mem3', 'mem4', 'uart', 'logic'];

export function BottomPanel() {
  const session = useIde((s) => s.session);
  const search = useProjectSearch();
  const tabs = session === 'off' ? EDIT_TABS : DEBUG_TABS;
  const fallback = session === 'off' ? 'build' : 'command';
  const active: PanelId = search.panel && tabs.includes(search.panel) ? search.panel : fallback;

  return (
    <section className="panel" aria-label="Panel">
      <Tabs
        value={active}
        onValueChange={(v) => {
          if (!v) return;
          setSearch({ panel: v as PanelId }, true);
          if (v.startsWith('mem')) void refreshMemory(Number(v.slice(3)) - 1);
        }}
        className="ptabs-root"
      >
        <div className="ptabs">
          <TabsList aria-label="Panel windows" className="ptabs-list">
            {tabs.map((t) => (
              <TabsTab key={t} value={t} className="ptab">
                {LABELS[t]}
              </TabsTab>
            ))}
          </TabsList>
          <span className="grow" />
          <Toolbar aria-label="Panel actions" className="ptabs-actions">
            <TbButton label="Close Panel" icon="close" keys="mod+j" onClick={() => ide.set({ panelOpen: false })} />
          </Toolbar>
        </div>
        {tabs.map((t) => (
          <TabsPanel key={t} value={t} className="pbody">
            {t === active ? <PanelBody id={t} /> : null}
          </TabsPanel>
        ))}
      </Tabs>
    </section>
  );
}

function PanelBody({ id }: { id: PanelId }) {
  switch (id) {
    case 'build':
      return <BuildOutput />;
    case 'command':
      return <CommandWindow />;
    case 'find':
      return (
        <div className="out">
          <FindResults />
        </div>
      );
    case 'uart':
      return <UartWindow />;
    case 'logic':
      return <LogicAnalyzer />;
    default:
      return <MemoryWindow idx={Number(id.slice(3)) - 1} />;
  }
}
