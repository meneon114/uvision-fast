import { Button } from '@ui-sdk/react/button';
import { toast } from '@ui-sdk/react/toast';
import { Tree, TreeItem, TreeItemLabel } from '@ui-sdk/react/tree';
import { useEffect, useRef, useState } from 'react';
import { host } from '../../host/client';
import type { ExampleInfo } from '../../host/types';
import { baseName, errMsg } from '../../lib/format';
import { openPath } from '../../state/ide';
import { askText, openDialog, useIde } from '../../state/store';
import { Icon } from '../Icon';
import { closeDialog, Modal } from './Modal';

const IMPORT_RE = /\.(uvprojx?|s|asm|inc|c|h|sct)$/i;

// Import dropped or picked files into the browser workspace and open the project.
export async function importFiles(files: File[]): Promise<void> {
  const keep = files.filter((f) => IMPORT_RE.test(f.name));
  if (!keep.length) {
    toast.warning('Nothing to import', { description: 'Drop .uvprojx, .s, .inc or .c files.' });
    return;
  }
  const proj = keep.find((f) => /\.uvprojx?$/i.test(f.name));
  const dir = `/Imported/${(proj ?? keep[0]).name.replace(/\.[^.]+$/, '')}`;
  try {
    const payload = await Promise.all(keep.map(async (f) => ({ name: f.name, content: await f.text() })));
    const entry = await host().host<string | null>('importFiles', { files: payload, dir });
    toast.success(`Imported ${keep.length} file${keep.length > 1 ? 's' : ''}`, { description: dir });
    if (entry) await openPath(entry);
  } catch (e) {
    toast.error('Import failed', { description: errMsg(e) });
  }
}

export function WorkspaceDialog() {
  const open = useIde((s) => s.dialog === 'workspace');
  const [files, setFiles] = useState<string[]>([]);
  const picker = useRef<HTMLInputElement>(null);
  const load = () =>
    host()
      .host<string[]>('listWorkspace')
      .then((f) => setFiles([...f].sort()))
      .catch(() => setFiles([]));
  useEffect(() => {
    if (open) void load();
  }, [open]);

  const projects = files.filter((f) => /\.uvprojx?$/i.test(f));
  const sources = files.filter((f) => !/\.uvprojx?$/i.test(f));

  const openOne = async (p: string) => {
    closeDialog();
    await openPath(p);
  };

  return (
    <Modal
      name="workspace"
      title="Browser Workspace"
      description="Projects and files saved in this browser (IndexedDB). They stay available offline."
      size="lg"
      footer={
        <>
          <Button
            onClick={async () => {
              const ok = await askText('Reset lab examples', 'Type RESET to restore the example projects to their original code', '');
              if (ok?.trim().toUpperCase() !== 'RESET') return;
              openDialog('workspace');
              await host().host('resetExamples');
              toast.success('Lab examples restored');
              void load();
            }}
          >
            <Icon name="discard" /> Reset Examples
          </Button>
          <span className="grow" />
          <input
            ref={picker}
            type="file"
            multiple
            hidden
            accept=".uvprojx,.uvproj,.s,.asm,.inc,.c,.h,.sct"
            onChange={async (e) => {
              const list = [...(e.currentTarget.files ?? [])];
              e.currentTarget.value = '';
              closeDialog();
              await importFiles(list);
            }}
          />
          <Button onClick={() => picker.current?.click()}>
            <Icon name="cloud-upload" /> Import Files...
          </Button>
          <Button data-variant="solid" onClick={closeDialog}>
            Close
          </Button>
        </>
      }
    >
      <div className="ws-cols">
        <section>
          <h3>Projects</h3>
          <FileList files={projects} icon="project" onOpen={openOne} onDeleted={load} />
        </section>
        <section>
          <h3>Source Files</h3>
          <FileList files={sources} icon="file-code" onOpen={openOne} onDeleted={load} />
        </section>
      </div>
      <p className="note">Tip: drop project folders or files anywhere on the window to import them.</p>
    </Modal>
  );
}

function FileList({ files, icon, onOpen, onDeleted }: { files: string[]; icon: string; onOpen: (p: string) => void; onDeleted: () => void }) {
  if (!files.length) return <p className="empty">None yet.</p>;
  return (
    <Tree aria-label="Files" className="tree ws-tree" mode="none" onAction={(v) => onOpen(String(v))}>
      {files.map((f) => (
        <TreeItem key={f} value={f}>
          <TreeItemLabel>
            <Icon name={icon} className="fi" />
            <span className="rn" title={f}>
              {baseName(f)}
            </span>
            <span className="rv plain">{f.slice(0, -baseName(f).length - 1)}</span>
            <button
              type="button"
              className="icon-btn row-action"
              aria-label={`Delete ${f}`}
              onClick={async (e) => {
                e.stopPropagation();
                const yes = await askText(`Delete ${baseName(f)}?`, 'Type DELETE to remove this file from the browser', '');
                openDialog('workspace');
                if (yes?.trim().toUpperCase() !== 'DELETE') return;
                await host().host('deleteWorkspaceFile', { path: f });
                onDeleted();
              }}
            >
              <Icon name="trash" />
            </button>
          </TreeItemLabel>
        </TreeItem>
      ))}
    </Tree>
  );
}

export function ExamplesDialog() {
  const open = useIde((s) => s.dialog === 'examples');
  const [items, setItems] = useState<ExampleInfo[]>([]);
  const hostKind = useIde((s) => s.hostKind);
  useEffect(() => {
    if (open && hostKind === 'web')
      void host()
        .host<ExampleInfo[]>('listExamples')
        .then(setItems)
        .catch(() => setItems([]));
  }, [open, hostKind]);

  return (
    <Modal
      name="examples"
      title="Lab Examples"
      description="RUET CSE 3106 starter projects for the STM32F103 Blue Pill."
      size="lg"
      footer={<Button onClick={closeDialog}>Close</Button>}
    >
      {hostKind !== 'web' ? (
        <p className="muted">Lab examples ship with the browser version. In VS Code, open a project folder with File, Open.</p>
      ) : (
        <ul className="examples">
          {items.map((ex) => (
            <li key={ex.id}>
              <button
                type="button"
                className="example"
                onClick={async () => {
                  closeDialog();
                  try {
                    const entry = await host().host<string>('installExample', { id: ex.id });
                    await openPath(entry);
                  } catch (e) {
                    toast.error('Could not open example', { description: errMsg(e) });
                  }
                }}
              >
                <span className="lab">{ex.lab}</span>
                <strong>{ex.title}</strong>
                <span className="muted">{ex.description}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </Modal>
  );
}
