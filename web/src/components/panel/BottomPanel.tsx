import { Tabs, TabsList, TabsPanel, TabsTab } from '@ui-sdk/react/tabs';
import { Toolbar } from '@ui-sdk/react/toolbar';
import type { PanelId } from '../../lib/search';
import { setSearch } from '../../lib/nav';
import { useProjectSearch } from '../../lib/useProjectSearch';
import { refreshMemory } from '../../state/ide';
import { ide, useIde } from '../../state/store';
import { TbButton, TbSize } from '../shell/TbButton';
import { Out } from './Out';
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
    <section className="flex-1 flex flex-col min-h-0 bg-bg" aria-label="Panel">
      <Tabs
        value={active}
        onValueChange={(v) => {
          if (!v) return;
          setSearch({ panel: v as PanelId }, true);
          if (v.startsWith('mem')) void refreshMemory(Number(v.slice(3)) - 1);
        }}
        className="flex-1 flex flex-col min-h-0"
      >
        <div className="h-8.75 flex items-center gap-1 pl-2.5 pr-2 flex-none">
          <TabsList aria-label="Panel windows" className="h-full min-w-0 border-0 overflow-x-auto no-scrollbar">
            {tabs.map((t) => (
              <TabsTab
                key={t}
                value={t}
                className="h-full m-0 px-2 border-b border-transparent text-muted text-2xs font-normal tracking-[0.03em] uppercase whitespace-nowrap data-selected:text-fg data-selected:border-acc"
              >
                {LABELS[t]}
              </TabsTab>
            ))}
          </TabsList>
          <span className="flex-1" />
          <TbSize value="sm">
            <Toolbar aria-label="Panel actions" className="flex items-center gap-px p-0 border-0 bg-transparent">
              <TbButton label="Close Panel" icon="close" keys="mod+j" onClick={() => ide.set({ panelOpen: false })} />
            </Toolbar>
          </TbSize>
        </div>
        {tabs.map((t) => (
          <TabsPanel key={t} value={t} className="flex-1 flex flex-col min-h-0">
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
        <Out>
          <FindResults />
        </Out>
      );
    case 'uart':
      return <UartWindow />;
    case 'logic':
      return <LogicAnalyzer />;
    default:
      return <MemoryWindow idx={Number(id.slice(3)) - 1} />;
  }
}
