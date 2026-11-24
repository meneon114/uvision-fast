import { createResizableController, Resizable, ResizableHandle, ResizablePanel } from '@ui-sdk/react/resizable';
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

const PHONE = '(max-width: 720px)';

// The whole IDE frame. Routes render it with their own editor content when
// they are pending or failed, so the chrome never disappears.
export function Workbench({ editor }: { editor?: ReactNode }) {
  const [main] = useState(createResizableController);
  const [center] = useState(createResizableController);
  const sideOpen = useIde((s) => s.sideOpen);
  const panelOpen = useIde((s) => s.panelOpen);
  const session = useIde((s) => s.session);
  const search = useProjectSearch();

  // Narrow screens start with the side bar and panel closed.
  useEffect(() => {
    if (window.matchMedia(PHONE).matches) ide.set({ sideOpen: false, panelOpen: false });
  }, []);

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
    <div className="app" data-session={session}>
      <GlobalShortcuts />
      <TitleBar />
      <KeilToolbar />
      <div className="body">
        <ActivityBar />
        <Resizable
          controller={main}
          className="main-split"
          autoSaveId="uv-main"
          onLayoutCommit={() => {
            const c = main.isCollapsed('side');
            if (c === sideOpen) ide.set({ sideOpen: !c });
          }}
        >
          <ResizablePanel id="side" defaultSize="268px" minSize="200px" maxSize="60%" collapsible className="side-panel">
            <SideBar view={view} />
          </ResizablePanel>
          <ResizableHandle aria-label="Resize side bar" className="handle" />
          <ResizablePanel id="center" minSize="280px" className="center">
            <Resizable
              orientation="vertical"
              controller={center}
              autoSaveId="uv-center"
              onLayoutCommit={() => {
                const c = center.isCollapsed('panel');
                if (c === panelOpen) ide.set({ panelOpen: !c });
              }}
            >
              <ResizablePanel id="editor" minSize="120px" className="editor-region">
                <EditorArea override={editor} />
              </ResizablePanel>
              <ResizableHandle aria-label="Resize panel" className="handle" />
              <ResizablePanel id="panel" defaultSize={28} minSize="96px" collapsible className="panel-region">
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
