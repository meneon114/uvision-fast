import { Select, SelectContent, SelectItem, SelectItemIndicator, SelectTrigger, SelectValue } from '../ui/select';
import { Toolbar, ToolbarButton, ToolbarGroup, ToolbarSeparator } from '@ui-sdk/react/toolbar';
import { exec, showView } from '../../lib/commands';
import { useProjectSearch } from '../../lib/useProjectSearch';
import { isRunning, showPanel } from '../../state/ide';
import { ide, useIde } from '../../state/store';
import { Icon } from '../Icon';
import { CmdButton, TbButton } from './TbButton';

const GROUP = 'flex items-center gap-px';
const SEP = 'w-px h-4.5 self-center mx-1.5 bg-border2';
const TARGET =
  'inline-flex items-center gap-1.5 h-6.5 w-auto min-w-0 px-2 mx-1 border border-border2 rounded-sm bg-bg text-fg text-sm cursor-pointer flex-none hover:border-muted phone:hidden';

export function KeilToolbar() {
  const session = useIde((s) => s.session);
  const busy = useIde((s) => s.busy);
  const building = useIde((s) => s.building);
  const project = useIde((s) => s.project);
  const disasmOpen = useIde((s) => s.disasmOpen);
  useIde((s) => s.dbg?.running);
  const search = useProjectSearch();
  const debugging = session !== 'off';
  const running = isRunning();
  const target = project?.options.target_name || 'Target 1';
  void busy;

  return (
    <Toolbar
      aria-label="Build and debug"
      className="h-9.5 flex items-center gap-px px-2 py-0 bg-chrome border-0 border-b border-border rounded-none flex-none overflow-hidden min-w-0"
      data-debug={debugging || undefined}
    >
      <ToolbarGroup aria-label="File" className={GROUP}>
        <CmdButton id="file.new" label="New File" />
        <CmdButton id="file.open" label="Open" />
        <CmdButton id="file.save" />
        <CmdButton id="file.saveAll" />
      </ToolbarGroup>
      <ToolbarSeparator className={SEP} />
      <ToolbarGroup aria-label="Edit" className={`${GROUP} compact:hidden`}>
        <CmdButton id="edit.undo" />
        <CmdButton id="edit.redo" />
        <CmdButton id="edit.findInFiles" />
      </ToolbarGroup>
      <ToolbarSeparator className={`${SEP} compact:hidden`} />
      <ToolbarGroup aria-label="Build" className={GROUP}>
        <CmdButton id="project.translate" label="Translate" />
        <CmdButton id="project.build" label={building ? 'Building...' : 'Build'} text="Build" />
        <CmdButton id="project.rebuild" label="Rebuild" />
        <CmdButton id="flash.download" label="Download" text="Load" />
      </ToolbarGroup>
      <Select value={target} onValueChange={() => {}}>
        <ToolbarButton render={<SelectTrigger />} aria-label="Select target" className={TARGET}>
          <Icon name="target" />
          <SelectValue />
        </ToolbarButton>
        <SelectContent>
          <SelectItem value={target}>
            {target}
            <SelectItemIndicator />
          </SelectItem>
        </SelectContent>
      </Select>
      <CmdButton id="project.options" label="Options for Target" />
      <ToolbarSeparator className={SEP} />
      <CmdButton id="debug.toggle" label="Start/Stop Debug Session" text="Debug" pressed={debugging} />
      {debugging ? (
        <>
          <ToolbarGroup aria-label="Run control" className={GROUP}>
            <CmdButton id="debug.reset" label="Reset CPU" />
            <CmdButton id="debug.run" tone="go" />
            <CmdButton id="debug.stop" tone="stop" pressed={running} />
            <CmdButton id="debug.step" />
            <CmdButton id="debug.stepOver" />
            <CmdButton id="debug.stepOut" />
            <CmdButton id="debug.runToCursor" />
          </ToolbarGroup>
          <ToolbarSeparator className={`${SEP} narrow:hidden`} />
          <ToolbarGroup aria-label="Debug windows" className={`${GROUP} narrow:hidden`}>
            <TbButton label="Command Window" icon="terminal" pressed={search.panel === 'command'} onClick={() => showPanel('command')} />
            <TbButton label="Disassembly Window" icon="file-binary" pressed={disasmOpen} onClick={() => ide.set((s) => ({ disasmOpen: !s.disasmOpen }))} />
            <TbButton label="Registers Window" icon="list-tree" pressed={search.view === 'debug'} onClick={() => showView('debug')} />
            <TbButton label="Memory Windows" icon="symbol-array" pressed={!!search.panel?.startsWith('mem')} onClick={() => showPanel('mem1')} />
            <TbButton label="Serial Window: UART #1" icon="output" pressed={search.panel === 'uart'} onClick={() => showPanel('uart')} />
            <TbButton label="Logic Analyzer" icon="pulse" pressed={search.panel === 'logic'} onClick={() => showPanel('logic')} />
            <TbButton label="System Viewer" icon="circuit-board" pressed={search.view === 'peripherals'} onClick={() => exec('view.peripherals')} />
          </ToolbarGroup>
        </>
      ) : null}
    </Toolbar>
  );
}
