import { Resizable, ResizableHandle, ResizablePanel } from '../ui/resizable';
import { Tabs, TabsList, TabsPanel, TabsTab } from '@ui-sdk/react/tabs';
import type { ReactNode } from 'react';
import { baseName, normPath, samePath } from '../../lib/format';
import { setSearch } from '../../lib/nav';
import { useProjectSearch } from '../../lib/useProjectSearch';
import { closeTab } from '../../state/ide';
import { isDirty, useIde } from '../../state/store';
import { Icon } from '../Icon';
import { CodeEditor } from './CodeEditor';
import { Disassembly } from './Disassembly';
import { GROUP, TAB, TAB_BAR, TAB_WRAP } from './tabs';
import { Welcome } from './Welcome';

function fileIcon(p: string): string {
  return /\.(c|h)$/i.test(p) ? 'file-code' : /\.(s|asm)$/i.test(p) ? 'file-binary' : 'file';
}

// The label above the cursor, for the breadcrumb (a Keil source has no PROC/ENDP here).
function enclosingLabel(content: string, line: number): string | null {
  const lines = content.split('\n');
  for (let i = Math.min(line, lines.length) - 1; i >= 0; i--) {
    const m = /^([A-Za-z_.$][\w.$]*)\b(?!\s+(EQU|RN)\b)/i.exec(lines[i]);
    if (m && !/^(AREA|END|EXPORT|IMPORT|THUMB|ALIGN|ENTRY)$/i.test(m[1])) return m[1];
  }
  return null;
}

function Breadcrumbs({ path }: { path: string }) {
  const project = useIde((s) => s.project);
  const content = useIde((s) => s.docs[normPath(path)]?.content ?? '');
  const line = useIde((s) => s.cursor.line);
  const group = project?.groups.find((g) => g.files.some((f) => samePath(f.abs_path, path)));
  const name = project?.project_file ? baseName(project.project_file).replace(/\.uvprojx?$/i, '') : null;
  const label = enclosingLabel(content, line);
  return (
    <nav
      className="h-6 flex items-center gap-1 px-3.5 text-sm text-muted flex-none overflow-hidden whitespace-nowrap [&_.codicon]:text-base"
      aria-label="Breadcrumbs"
    >
      {name ? (
        <>
          <span>{name}</span>
          <Icon name="chevron-right" />
        </>
      ) : null}
      {group ? (
        <>
          <span>{group.name}</span>
          <Icon name="chevron-right" />
        </>
      ) : null}
      <span>{baseName(path)}</span>
      {label ? (
        <>
          <Icon name="chevron-right" />
          <Icon name="symbol-method" className="text-acc" />
          <span>{label}</span>
        </>
      ) : null}
    </nav>
  );
}

function EditorGroup({ children }: { children?: ReactNode }) {
  const tabs = useIde((s) => s.tabs);
  const docs = useIde((s) => s.docs);
  const search = useProjectSearch();
  const active = tabs.find((t) => samePath(t, search.file)) ?? null;
  const loaded = active ? !!docs[normPath(active)] : false;

  if (children) return <div className={GROUP}>{children}</div>;
  if (!active) {
    return (
      <div className={GROUP}>
        <Welcome />
      </div>
    );
  }

  return (
    <div className={GROUP}>
      <Tabs value={active} onValueChange={(v) => v && setSearch({ file: v, line: undefined })} className="flex-1 flex flex-col min-h-0">
        <TabsList aria-label="Open files" className={TAB_BAR}>
          {tabs.map((t) => {
            const dirty = isDirty(docs[normPath(t)]);
            return (
              <div className={TAB_WRAP} key={t} data-active={t === active || undefined}>
                <TabsTab
                  value={t}
                  className={TAB}
                  title={t}
                  onAuxClick={(e) => {
                    if (e.button === 1) closeTab(t);
                  }}
                >
                  <Icon name={fileIcon(t)} className="text-asm text-md" />
                  <span>{baseName(t)}</span>
                  {dirty ? <span className="size-2 mx-0.5 rounded-full bg-fg2" aria-label="Unsaved changes" /> : null}
                </TabsTab>
                <button
                  type="button"
                  className="grid place-items-center size-5 mr-1.5 p-0 border-0 rounded-xs bg-transparent text-muted cursor-pointer opacity-0 group-hover/tab:opacity-100 group-data-active/tab:opacity-100 hover:bg-hover"
                  tabIndex={-1}
                  aria-label={`Close ${baseName(t)}`}
                  onClick={() => closeTab(t)}
                >
                  <Icon name="close" className="text-md" />
                </button>
              </div>
            );
          })}
        </TabsList>
        {tabs.map((t) => (
          <TabsPanel key={t} value={t} className="flex-1 flex flex-col min-h-0">
            {t === active ? (
              <>
                <Breadcrumbs path={t} />
                {loaded ? <CodeEditor path={t} line={search.line} /> : <div className="p-4 text-muted">Loading {baseName(t)}...</div>}
              </>
            ) : null}
          </TabsPanel>
        ))}
      </Tabs>
    </div>
  );
}

export function EditorArea({ override }: { override?: ReactNode }) {
  const session = useIde((s) => s.session);
  const disasmOpen = useIde((s) => s.disasmOpen);
  const showDisasm = session === 'on' && disasmOpen && !override;

  if (!showDisasm) return <EditorGroup>{override}</EditorGroup>;
  return (
    <Resizable orientation="horizontal" className="flex-1 min-h-0" autoSaveId="uv-editor-split">
      <ResizablePanel id="source" minSize="240px">
        <EditorGroup />
      </ResizablePanel>
      <ResizableHandle aria-label="Resize disassembly" />
      <ResizablePanel id="disasm" defaultSize="38%" minSize="220px">
        <Disassembly />
      </ResizablePanel>
    </Resizable>
  );
}
