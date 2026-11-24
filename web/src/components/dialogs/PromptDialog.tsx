import { Button } from '@ui-sdk/react/button';
import { useEffect, useState } from 'react';
import { ide, useIde } from '../../state/store';
import { Modal, TextField } from './Modal';

export function PromptDialog() {
  const req = useIde((s) => s.prompt);
  const [value, setValue] = useState('');
  useEffect(() => setValue(req?.value ?? ''), [req]);

  const finish = (v: string | null) => {
    const r = ide.get().prompt;
    ide.set({ prompt: null, dialog: null });
    r?.resolve(v);
  };

  return (
    <Modal
      name="prompt"
      title={req?.title ?? ''}
      size="sm"
      onClose={() => {
        const r = ide.get().prompt;
        if (r) {
          ide.set({ prompt: null });
          r.resolve(null);
        }
      }}
      footer={
        <>
          <Button onClick={() => finish(null)}>Cancel</Button>
          <Button data-variant="solid" type="submit" form="prompt-form">
            OK
          </Button>
        </>
      }
    >
      <form
        id="prompt-form"
        onSubmit={(e) => {
          e.preventDefault();
          finish(value);
        }}
      >
        <TextField label={req?.label ?? ''} value={value} onChange={setValue} mono autoFocus />
      </form>
    </Modal>
  );
}
