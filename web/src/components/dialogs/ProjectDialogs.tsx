import { Button } from '@ui-sdk/react/button';
import { toast } from '@ui-sdk/react/toast';
import { useEffect, useState } from 'react';
import { host } from '../../host/client';
import { errMsg } from '../../lib/format';
import { addFileToProject, newProject } from '../../state/ide';
import { useIde } from '../../state/store';
import { closeDialog, Modal, SelectField, TextField } from './Modal';

export function NewProjectDialog() {
  const open = useIde((s) => s.dialog === 'newProject');
  const hostKind = useIde((s) => s.hostKind);
  const [name, setName] = useState('LabProject');
  const [dir, setDir] = useState('/Workspace/LabProject');
  const [dirTouched, setDirTouched] = useState(false);
  const [device, setDevice] = useState('STM32F103C8');
  const [template, setTemplate] = useState<'asm' | 'c'>('asm');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open) {
      setName('LabProject');
      setDir(hostKind === 'web' ? '/Workspace/LabProject' : '');
      setDirTouched(false);
    }
  }, [open, hostKind]);

  return (
    <Modal
      name="newProject"
      title="Create New µVision Project"
      size="md"
      footer={
        <>
          <Button onClick={closeDialog}>Cancel</Button>
          <Button
            data-variant="solid"
            disabled={!dir.trim() || !name.trim() || busy}
            onClick={async () => {
              setBusy(true);
              try {
                await newProject(dir.trim(), name.trim(), device, template);
                closeDialog();
              } catch (e) {
                toast.error('Project not created', { description: errMsg(e) });
              } finally {
                setBusy(false);
              }
            }}
          >
            Create
          </Button>
        </>
      }
    >
      <div className="form-grid">
        <TextField
          label="Project Name"
          value={name}
          autoFocus
          onChange={(v) => {
            setName(v);
            if (hostKind === 'web' && !dirTouched) setDir(`/Workspace/${v.trim() || 'LabProject'}`);
          }}
        />
        <div className="field-row">
          <TextField
            label="Folder"
            value={dir}
            mono
            onChange={(v) => {
              setDir(v);
              setDirTouched(true);
            }}
          />
          {hostKind === 'vscode' ? (
            <Button
              onClick={async () => {
                const d = await host().host<string | null>('pickNewProjectDir');
                if (d) {
                  setDir(d);
                  setDirTouched(true);
                }
              }}
            >
              Browse...
            </Button>
          ) : null}
        </div>
        <SelectField
          label="Device"
          value={device}
          options={[
            ['STM32F103C8', 'STM32F103C8 (Blue Pill)'],
            ['STM32F103RB', 'STM32F103RB'],
            ['STM32F100RB', 'STM32F100RB'],
            ['ARMCM3', 'ARMCM3'],
          ]}
          onChange={setDevice}
        />
        <SelectField
          label="Template"
          value={template}
          options={[
            ['asm', 'ARM Assembly (.s) with startup skeleton'],
            ['c', 'STM32 C (.c), VS Code only'],
          ]}
          onChange={setTemplate}
        />
      </div>
    </Modal>
  );
}

export function AddItemDialog() {
  const open = useIde((s) => s.dialog === 'addItem');
  const group = useIde((s) => s.project?.groups[0]?.name ?? 'Source Group 1');
  const [name, setName] = useState('main.s');
  const [type, setType] = useState<'asm' | 'c' | 'empty'>('asm');
  useEffect(() => {
    if (open) setName(type === 'c' ? 'main.c' : type === 'asm' ? 'main.s' : 'defs.inc');
  }, [open]);

  const submit = async () => {
    if (!name.trim()) return;
    closeDialog();
    await addFileToProject(name.trim(), type);
  };

  return (
    <Modal
      name="addItem"
      title={`Add New Item to Group '${group}'`}
      size="sm"
      footer={
        <>
          <Button onClick={closeDialog}>Cancel</Button>
          <Button data-variant="solid" type="submit" form="add-item-form" disabled={!name.trim()}>
            Add
          </Button>
        </>
      }
    >
      <form
        id="add-item-form"
        className="form-grid one"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <SelectField
          label="Type"
          value={type}
          options={[
            ['asm', 'Asm File (.s) with course skeleton'],
            ['c', 'C File (.c)'],
            ['empty', 'Empty file'],
          ]}
          onChange={(t) => {
            setType(t);
            setName((n) => n.replace(/\.(s|c|inc)$/i, '') + (t === 'asm' ? '.s' : t === 'c' ? '.c' : '.inc'));
          }}
        />
        <TextField label="File Name" value={name} onChange={setName} mono autoFocus />
      </form>
    </Modal>
  );
}
