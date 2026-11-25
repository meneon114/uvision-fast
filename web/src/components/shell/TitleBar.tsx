import {
  Menu,
  MenuCheckboxItem,
  MenuContent,
  MenuGroup,
  MenuGroupLabel,
  MenuItem,
  MenuItemIcon,
  MenuItemIndicator,
  MenuRadioGroup,
  MenuRadioItem,
  MenuSeparator,
  MenuSub,
  MenuSubContent,
  MenuSubTrigger,
  MenuTrigger,
} from '../ui/menu';
import { Menubar } from '@ui-sdk/react/menubar';
import { Toolbar } from '@ui-sdk/react/toolbar';
import { cmd, exec, isEnabled } from '../../lib/commands';
import { baseName } from '../../lib/format';
import { openPath, setTheme } from '../../state/ide';
import { ide, openDialog, useIde, type Theme } from '../../state/store';
import { Icon } from '../Icon';
import { host } from '../../host/client';
import { setSearch } from '../../lib/nav';
import { TbButton, TbSize } from './TbButton';

const TOP = 'h-6.5 px-2 border-0 rounded-sm bg-transparent text-fg2 text-base cursor-pointer hover:bg-hover data-open:bg-hover compact:px-1.5';

// A top-level menu in the menu bar.
function Top({ label }: { label: string }) {
  return <MenuTrigger className={TOP}>{label}</MenuTrigger>;
}

function Item({ id, label }: { id: string; label?: string }) {
  const c = cmd(id);
  return (
    <MenuItem value={id} shortcut={c.keys} disabled={!isEnabled(id)} onSelect={() => void c.run()}>
      {c.icon ? (
        <MenuItemIcon>
          <Icon name={c.icon} className="text-lg" />
        </MenuItemIcon>
      ) : null}
      {label ?? c.title}
    </MenuItem>
  );
}

function Check({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <MenuCheckboxItem value={label} checked={checked} onCheckedChange={onChange}>
      <MenuItemIndicator />
      {label}
    </MenuCheckboxItem>
  );
}

const PERIPHERALS = ['GPIOA', 'GPIOB', 'GPIOC', 'TIM2', 'TIM3', 'USART1', 'SysTick', 'RCC'];

export function TitleBar() {
  // Subscribing re-renders the menus so enabled states stay current.
  const session = useIde((s) => s.session);
  useIde((s) => s.busy);
  const project = useIde((s) => s.project);
  const projectPath = useIde((s) => s.projectPath);
  const recent = useIde((s) => s.recent);
  const theme = useIde((s) => s.theme);
  const sideOpen = useIde((s) => s.sideOpen);
  const panelOpen = useIde((s) => s.panelOpen);
  const disasmOpen = useIde((s) => s.disasmOpen);
  const tabs = useIde((s) => s.tabs);
  const pwa = useIde((s) => s.pwa);
  const hostKind = useIde((s) => s.hostKind);

  const title = projectPath ? `${baseName(projectPath).replace(/\.uvprojx?$/i, '')}  ·  ${project?.options.device ?? 'STM32F103C8'}` : 'Search commands';

  return (
    <header className="h-8.5 flex items-center gap-0.5 px-1.5 bg-chrome border-b border-border flex-none min-w-0" data-session={session}>
      <span className="w-7 grid place-items-center flex-none text-acc" aria-hidden="true">
        <Icon name="chip" />
      </span>
      <Menubar aria-label="Main menu" className="gap-0 p-0 border-0 bg-transparent min-w-0 overflow-hidden flex-[0_1_auto] phone:*:nth-[n+4]:hidden">
        <Menu>
          <Top label="File" />
          <MenuContent>
            <Item id="file.new" />
            <Item id="file.open" />
            <Item id="file.examples" />
            <MenuSub>
              <MenuSubTrigger disabled={recent.length === 0}>
                <MenuItemIcon>
                  <Icon name="history" className="text-lg" />
                </MenuItemIcon>
                Recent Projects
              </MenuSubTrigger>
              <MenuSubContent>
                {recent.map((p) => (
                  <MenuItem key={p} value={p} onSelect={() => void openPath(p)}>
                    {p}
                  </MenuItem>
                ))}
              </MenuSubContent>
            </MenuSub>
            <MenuSeparator />
            <Item id="file.save" />
            <Item id="file.saveAll" />
            <Item id="file.close" />
            <MenuSeparator />
            <Item id="file.export" />
            {hostKind === 'web' ? <Item id="tools.workspace" /> : null}
            {hostKind === 'web' && pwa.canInstall ? (
              <MenuItem value="install" onSelect={() => void host().host('installPwa')}>
                <MenuItemIcon>
                  <Icon name="desktop-download" className="text-lg" />
                </MenuItemIcon>
                Install as App
              </MenuItem>
            ) : null}
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="Edit" />
          <MenuContent>
            <Item id="edit.undo" />
            <Item id="edit.redo" />
            <MenuSeparator />
            <Item id="edit.find" />
            <Item id="edit.findInFiles" />
            <MenuSeparator />
            <Item id="edit.toggleBp" />
            <Item id="edit.killBps" />
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="View" />
          <MenuContent>
            <Item id="view.project" />
            <Item id="view.find" />
            <Item id="view.debug" />
            <Item id="view.peripherals" />
            <Item id="view.flash" />
            <MenuSeparator />
            <Check label="Side Bar" checked={sideOpen} onChange={(v) => ide.set({ sideOpen: v })} />
            <Check label="Panel" checked={panelOpen} onChange={(v) => ide.set({ panelOpen: v })} />
            <Check label="Disassembly Window" checked={disasmOpen} onChange={(v) => ide.set({ disasmOpen: v })} />
            <MenuSeparator />
            <Item id="view.build" />
            <Item id="view.command" />
            <MenuSub>
              <MenuSubTrigger>
                <MenuItemIcon>
                  <Icon name="symbol-array" className="text-lg" />
                </MenuItemIcon>
                Memory Windows
              </MenuSubTrigger>
              <MenuSubContent>
                <Item id="view.mem1" />
                <Item id="view.mem2" />
                <Item id="view.mem3" />
                <Item id="view.mem4" />
              </MenuSubContent>
            </MenuSub>
            <Item id="view.uart" />
            <Item id="view.logic" />
            <MenuSeparator />
            <MenuGroup>
              <MenuGroupLabel>Theme</MenuGroupLabel>
              <MenuRadioGroup value={theme} onValueChange={(v) => setTheme(v as Theme)}>
                <MenuRadioItem value="system">
                  <MenuItemIndicator />
                  Follow System
                </MenuRadioItem>
                <MenuRadioItem value="light">
                  <MenuItemIndicator />
                  Light
                </MenuRadioItem>
                <MenuRadioItem value="dark">
                  <MenuItemIndicator />
                  Dark
                </MenuRadioItem>
              </MenuRadioGroup>
            </MenuGroup>
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="Project" />
          <MenuContent>
            <Item id="project.new" />
            <Item id="file.open" label="Open Project..." />
            <MenuSeparator />
            <Item id="project.addItem" />
            <Item id="project.addExisting" />
            <MenuSeparator />
            <Item id="project.options" />
            <MenuSeparator />
            <Item id="project.translate" />
            <Item id="project.build" />
            <Item id="project.rebuild" />
            <MenuSeparator />
            <Item id="project.packs" />
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="Flash" />
          <MenuContent>
            <Item id="flash.download" />
            <Item id="flash.export" />
            <MenuSeparator />
            <Item id="view.flash" label="Flash Settings" />
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="Debug" />
          <MenuContent>
            <Item id="debug.toggle" />
            <MenuSeparator />
            <Item id="debug.reset" />
            <Item id="debug.run" />
            <Item id="debug.stop" />
            <Item id="debug.step" />
            <Item id="debug.stepOver" />
            <Item id="debug.stepOut" />
            <Item id="debug.runToCursor" />
            <MenuSeparator />
            <Item id="edit.toggleBp" />
            <Item id="edit.killBps" />
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="Peripherals" />
          <MenuContent>
            <Item id="view.peripherals" label="System Viewer" />
            <MenuSeparator />
            {PERIPHERALS.map((p) => (
              <MenuItem key={p} value={p} onSelect={() => exec('view.peripherals')}>
                {p}
              </MenuItem>
            ))}
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="Tools" />
          <MenuContent>
            <Item id="tools.palette" />
            <Item id="tools.toolchain" />
            <Item id="project.packs" />
            {hostKind === 'web' ? <Item id="tools.workspace" /> : null}
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="Window" />
          <MenuContent>
            <Item id="view.sidebar" />
            <Item id="view.panel" />
            <MenuItem value="reset-layout" onSelect={() => ide.set({ sideOpen: true, panelOpen: true, disasmOpen: true })}>
              <MenuItemIcon>
                <Icon name="layout" className="text-lg" />
              </MenuItemIcon>
              Reset View
            </MenuItem>
            {tabs.length ? <MenuSeparator /> : null}
            {tabs.map((t) => (
              <MenuItem key={t} value={t} onSelect={() => setFile(t)}>
                {baseName(t)}
              </MenuItem>
            ))}
          </MenuContent>
        </Menu>
        <Menu>
          <Top label="Help" />
          <MenuContent>
            <Item id="help.shortcuts" />
            <Item id="file.examples" label="Lab Examples" />
            <Item id="help.about" />
          </MenuContent>
        </Menu>
      </Menubar>
      <span className="flex-1" />
      <button
        type="button"
        className="w-[min(420px,36vw)] h-6 flex-[0_1_auto] min-w-30 flex items-center justify-center gap-2 px-2.5 border border-border2 rounded-md bg-cmdc text-muted text-sm cursor-pointer overflow-hidden whitespace-nowrap text-ellipsis hover:border-muted hover:text-fg2 active:translate-y-px compact:w-auto compact:flex-auto phone:flex-none phone:w-8 phone:min-w-8 phone:p-0"
        onClick={() => openDialog('palette')}
        aria-label="Search commands"
        aria-keyshortcuts="Control+Shift+P"
      >
        <Icon name="search" />
        <span className="overflow-hidden text-ellipsis phone:hidden">{title}</span>
      </button>
      <span className="flex-1" />
      <TbSize value="sm">
        <Toolbar aria-label="Layout" className="flex flex-none flex-nowrap items-center gap-px p-0 border-0 bg-transparent">
          <TbButton
            label="Toggle Side Bar"
            keys="mod+b"
            icon={sideOpen ? 'layout-sidebar-left' : 'layout-sidebar-left-off'}
            pressed={sideOpen}
            onClick={() => exec('view.sidebar')}
          />
          <TbButton
            label="Toggle Panel"
            keys="mod+j"
            icon={panelOpen ? 'layout-panel' : 'layout-panel-off'}
            pressed={panelOpen}
            onClick={() => exec('view.panel')}
          />
          <TbButton
            label={`Theme: ${theme === 'system' ? 'follow system' : theme}`}
            icon={theme === 'dark' ? 'moon' : theme === 'light' ? 'sun' : 'color-mode'}
            onClick={() => exec('view.theme')}
          />
        </Toolbar>
      </TbSize>
    </header>
  );
}

function setFile(file: string) {
  setSearch({ file, line: undefined });
}
