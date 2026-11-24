import { Select, SelectContent, SelectItem, SelectItemIndicator, SelectTrigger, SelectValue } from '@ui-sdk/react/select';
import { Toolbar, ToolbarButton, ToolbarGroup, ToolbarSeparator } from '@ui-sdk/react/toolbar';
import { exec, showView } from '../../lib/commands';
import { useProjectSearch } from '../../lib/useProjectSearch';
import { isRunning, showPanel } from '../../state/ide';
import { ide, useIde } from '../../state/store';
import { Icon } from '../Icon';
import { CmdButton, TbButton } from './TbButton';

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
    <Toolbar aria-label="Build and debug" className="keil-toolbar" data-debug={debugging || undefined}>
      <ToolbarGroup aria-label="File">
        <CmdButton id="file.new" label="New File" />
        <CmdButton id="file.open" label="Open" />
        <CmdButton id="file.save" />
        <CmdButton id="file.saveAll" />
      </ToolbarGroup>
      <ToolbarSeparator />
      <ToolbarGroup aria-label="Edit" className="hide-sm">
        <CmdButton id="edit.undo" />
        <CmdButton id="edit.redo" />
        <CmdButton id="edit.findInFiles" />
      </ToolbarGroup>
      <ToolbarSeparator className="hide-sm" />
      <ToolbarGroup aria-label="Build">
        <CmdButton id="project.translate" label="Translate" />
        <CmdButton id="project.build" label={building ? 'Building...' : 'Build'} text="Build" />
        <CmdButton id="project.rebuild" label="Rebuild" />
        <CmdButton id="flash.download" label="Download" text="Load" />
      </ToolbarGroup>
      <Select value={target} onValueChange={() => {}}>
        <ToolbarButton render={<SelectTrigger />} aria-label="Select target" className="tb-select">
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
      <ToolbarSeparator />
      <CmdButton id="debug.toggle" label="Start/Stop Debug Session" text="Debug" pressed={debugging} />
      {debugging ? (
        <>
          <ToolbarGroup aria-label="Run control">
            <CmdButton id="debug.reset" label="Reset CPU" />
            <CmdButton id="debug.run" tone="go" />
            <CmdButton id="debug.stop" tone="stop" pressed={running} />
            <CmdButton id="debug.step" />
            <CmdButton id="debug.stepOver" />
            <CmdButton id="debug.stepOut" />
            <CmdButton id="debug.runToCursor" />
          </ToolbarGroup>
          <ToolbarSeparator className="hide-md" />
          <ToolbarGroup aria-label="Debug windows" className="hide-md">
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
