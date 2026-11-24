import { Button } from '@ui-sdk/react/button';
import { Progress, ProgressIndicator, ProgressLabel, ProgressTrack, ProgressValue } from '@ui-sdk/react/progress';
import { toast } from '@ui-sdk/react/toast';
import { useEffect, useState } from 'react';
import { host } from '../../host/client';
import type { BuildArtifacts, PackInfo } from '../../host/types';
import { errMsg } from '../../lib/format';
import { ide, useIde } from '../../state/store';
import { Icon } from '../Icon';
import { closeDialog, Modal } from './Modal';

export function ExportDialog() {
  const open = useIde((s) => s.dialog === 'export');
  const build = useIde((s) => s.build);
  const hostKind = useIde((s) => s.hostKind);
  const [arts, setArts] = useState<BuildArtifacts | null>(null);
  useEffect(() => {
    if (open && hostKind === 'web')
      void host()
        .rpc<BuildArtifacts>('build.artifacts')
        .then(setArts)
        .catch(() => setArts(null));
  }, [open, hostKind]);

  const name = arts?.output_name || 'firmware';
  const dl = (file: string, data: { text?: string; b64?: string }, mime: string) => void host().host('downloadFile', { name: file, mime, ...data });

  return (
    <Modal
      name="export"
      title="Export Firmware"
      description="Download the build output to program the board with another tool."
      size="sm"
      footer={<Button onClick={closeDialog}>Close</Button>}
    >
      {hostKind === 'web' ? (
        !arts?.has_artifacts ? (
          <p className="muted">Build the target (F7) first.</p>
        ) : (
          <div className="stack">
            <Button disabled={!arts.hex_text} onClick={() => dl(`${name}.hex`, { text: arts.hex_text }, 'text/plain')}>
              <Icon name="file-binary" /> {name}.hex <span className="muted">Intel HEX</span>
            </Button>
            <Button disabled={!arts.bin_b64} onClick={() => dl(`${name}.bin`, { b64: arts.bin_b64 }, 'application/octet-stream')}>
              <Icon name="file-binary" /> {name}.bin <span className="muted">raw, load at 0x08000000</span>
            </Button>
            <Button disabled={!arts.elf_b64} onClick={() => dl(`${name}.axf`, { b64: arts.elf_b64 }, 'application/octet-stream')}>
              <Icon name="file-binary" /> {name}.axf <span className="muted">ELF with symbols</span>
            </Button>
          </div>
        )
      ) : (
        <dl className="kv">
          {[
            ['AXF', build?.axf_path],
            ['HEX', build?.hex_path],
            ['BIN', build?.bin_path],
          ].map(([k, v]) => (
            <div key={k}>
              <dt>{k}</dt>
              <dd className="mono">{v || 'not created (see Options, Output)'}</dd>
            </div>
          ))}
        </dl>
      )}
    </Modal>
  );
}

export function FlashDialog() {
  const open = useIde((s) => s.dialog === 'flash');
  const log = useIde((s) => s.flashLog);
  const [supported, setSupported] = useState<boolean | null>(null);
  const [running, setRunning] = useState(false);
  useEffect(() => {
    if (open)
      void host()
        .host<boolean>('webSerialSupported')
        .then(setSupported)
        .catch(() => setSupported(false));
  }, [open]);

  const start = async () => {
    setRunning(true);
    ide.set({ flashLog: { percent: 0, lines: [] }, status: 'Downloading to flash over USART1...' });
    try {
      await host().host('flashWebSerial');
      ide.set({ status: 'Flash Load finished' });
      toast.success('Flash Load finished', { description: 'Set BOOT0 back to 0 and press reset to run.' });
    } catch (e) {
      ide.set((s) => ({ status: 'Flash Download failed', flashLog: { ...s.flashLog, lines: [...s.flashLog.lines, `Error: ${errMsg(e)}`] } }));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Modal
      name="flash"
      title="Download to Flash"
      description="Programs the Blue Pill through its built-in serial bootloader with a USB to UART adapter."
      size="md"
      footer={
        <>
          <Button onClick={closeDialog} disabled={running}>
            Close
          </Button>
          <Button data-variant="solid" onClick={start} disabled={!supported || running}>
            <Icon name="plug" /> {running ? 'Downloading...' : 'Connect and Download'}
          </Button>
        </>
      }
    >
      {supported === false ? (
        <p className="note warn">This browser has no Web Serial. Use Chrome or Edge on a desktop, or use Flash, Export Firmware and an external tool.</p>
      ) : null}
      <ol className="steps">
        <li>Wire the adapter: TX to PA10, RX to PA9, GND to GND.</li>
        <li>Set BOOT0 to 1, then press reset.</li>
        <li>Press Connect and Download, then choose the adapter's port.</li>
      </ol>
      <Progress value={log.percent} className="progress">
        <ProgressLabel>Progress</ProgressLabel>
        <ProgressValue />
        <ProgressTrack>
          <ProgressIndicator />
        </ProgressTrack>
      </Progress>
      <pre className="out mono flash-log" role="log" aria-label="Flash log">
        {log.lines.join('\n') || 'Waiting to start.'}
      </pre>
    </Modal>
  );
}

export function PacksDialog() {
  const open = useIde((s) => s.dialog === 'packs');
  const hostKind = useIde((s) => s.hostKind);
  const [packs, setPacks] = useState<PackInfo[] | null>(null);
  const load = () =>
    host()
      .rpc<PackInfo[]>('pack.list')
      .then(setPacks)
      .catch(() => setPacks([]));
  useEffect(() => {
    if (open) void load();
  }, [open]);

  return (
    <Modal
      name="packs"
      title="Pack Installer"
      size="md"
      footer={
        <>
          {hostKind === 'vscode' ? (
            <Button
              onClick={async () => {
                try {
                  await host().host('installPack');
                  void load();
                } catch (e) {
                  toast.error('Pack not installed', { description: errMsg(e) });
                }
              }}
            >
              <Icon name="package" /> Install .pack File...
            </Button>
          ) : null}
          <span className="grow" />
          <Button data-variant="solid" onClick={closeDialog}>
            Close
          </Button>
        </>
      }
    >
      {packs === null ? (
        <p className="muted">Loading...</p>
      ) : packs.length === 0 ? (
        <p className="muted">No device packs installed. The built-in STM32F1 device support is always available.</p>
      ) : (
        <table className="grid-table">
          <thead>
            <tr>
              <th scope="col">Vendor</th>
              <th scope="col">Pack</th>
              <th scope="col">Version</th>
            </tr>
          </thead>
          <tbody>
            {packs.map((p) => (
              <tr key={`${p.vendor}.${p.name}`}>
                <td>{p.vendor}</td>
                <td>{p.name}</td>
                <td className="mono">{p.version}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Modal>
  );
}
