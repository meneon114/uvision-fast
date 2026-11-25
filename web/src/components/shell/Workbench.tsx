import { createResizableController, Resizable, ResizableHandle, ResizablePanel } from '../ui/resizable';
import { useEffect, useState, type ReactNode } from 'react';
import { useIde, ide } from '../../state/store';
import { useProjectSearch } from '../../lib/useProjectSearch';
import { EditorArea } from '../editor/EditorArea';
import { BottomPanel } from '../panel/BottomPanel';
import { SideBar } from '../side/SideBar';
import { Dialogs } from '../dialogs/Dialogs';
import { ActivityBar } from './ActivityBar';
import { GlobalShortcuts } from './GlobalShortcuts';
import { KeilToolbar } from './KeilToolbar';
import { StatusBar } from './StatusBar';
import { TitleBar } from './TitleBar';

// While files are dragged over the window (`data-drop` on <html>), a dashed overlay says what dropping does.
const DROP_HINT =
  "in-data-drop:after:content-['Drop_files_to_import_them_into_the_browser_workspace'] in-data-drop:after:fixed in-data-drop:after:inset-2 in-data-drop:after:z-50 in-data-drop:after:grid in-data-drop:after:place-items-center in-data-drop:after:border-2 in-data-drop:after:border-dashed in-data-drop:after:border-acc in-data-drop:after:rounded-md in-data-drop:after:bg-bg/85 in-data-drop:after:text-acc in-data-drop:after:text-lg in-data-drop:after:pointer-events-none";

// The whole IDE frame. Routes render it with their own editor content when
// they are pending or failed, so the chrome never disappears.
export function Workbench({ editor }: { editor?: ReactNode }) {
  const [main] = useState(createResizableController);
  const [center] = useState(createResizableController);
  const sideOpen = useIde((s) => s.sideOpen);
  const panelOpen = useIde((s) => s.panelOpen);
  const session = useIde((s) => s.session);
  const search = useProjectSearch();

  useEffect(() => {
    if (sideOpen) main.expand('side');
    else main.collapse('side');
  }, [sideOpen, main]);

  useEffect(() => {
    if (panelOpen) center.expand('panel');
    else center.collapse('panel');
  }, [panelOpen, center]);

  const view = search.view ?? (session === 'off' ? 'project' : 'debug');

  return (
    <div className={`h-dvh flex flex-col bg-chrome text-fg2 overflow-hidden ${DROP_HINT}`} data-session={session}>
      <GlobalShortcuts />
      <TitleBar />
      <KeilToolbar />
      <div className="flex-1 flex min-h-0 min-w-0">
        <ActivityBar />
        <Resizable
          controller={main}
          className="flex-1 min-w-0"
          autoSaveId="uv-main"
          onLayoutCommit={() => {
            const c = main.isCollapsed('side');
            if (c === sideOpen) ide.set({ sideOpen: !c });
          }}
        >
          <ResizablePanel id="side" defaultSize="268px" minSize="200px" maxSize="60%" collapsible className="bg-chrome">
            <SideBar view={view} />
          </ResizablePanel>
          <ResizableHandle aria-label="Resize side bar" />
          <ResizablePanel id="center" minSize="280px" className="bg-bg">
            <Resizable
              orientation="vertical"
              controller={center}
              autoSaveId="uv-center"
              onLayoutCommit={() => {
                const c = center.isCollapsed('panel');
                if (c === panelOpen) ide.set({ panelOpen: !c });
              }}
            >
              <ResizablePanel id="editor" minSize="120px" className="bg-bg">
                <EditorArea override={editor} />
              </ResizablePanel>
              <ResizableHandle aria-label="Resize panel" />
              <ResizablePanel id="panel" defaultSize={28} minSize="96px" collapsible className="bg-bg">
                <BottomPanel />
              </ResizablePanel>
            </Resizable>
          </ResizablePanel>
        </Resizable>
      </div>
      <StatusBar />
      <Dialogs />
    </div>
  );
}
