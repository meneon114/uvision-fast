import { Button } from '../ui/Button';
import { Note } from '../ui/kit';
import { Tabs, TabsList, TabsPanel, TabsTab } from '@ui-sdk/react/tabs';
import { useEffect, useState } from 'react';
import type { TargetOptions } from '../../host/types';
import { hex32, parseNum } from '../../lib/format';
import { saveOptions } from '../../state/ide';
import { useIde } from '../../state/store';
import { CheckField, closeDialog, formGrid, Modal, SelectField, TextField } from './Modal';

const SEG_TAB = 'h-7.5 font-normal text-muted data-selected:border-acc data-selected:text-fg';

const DEVICES: [string, string][] = [
  ['STM32F103C8', 'STM32F103C8 (Blue Pill, 64 KB)'],
  ['STM32F103RB', 'STM32F103RB (128 KB)'],
  ['STM32F100RB', 'STM32F100RB (Value Line)'],
  ['ARMCM3', 'ARMCM3 (generic Cortex-M3)'],
];

type Draft = Omit<TargetOptions, 'xtal_hz' | 'ro_base' | 'ro_size' | 'rw_base' | 'rw_size' | 'c_defines' | 'c_includes' | 'a_defines' | 'a_includes'> & {
  xtal: string;
  ro_base: string;
  ro_size: string;
  rw_base: string;
  rw_size: string;
  c_defines: string;
  c_includes: string;
  a_defines: string;
  a_includes: string;
};

function toDraft(o: TargetOptions): Draft {
  return {
    ...o,
    xtal: String(o.xtal_hz / 1e6),
    ro_base: hex32(o.ro_base),
    ro_size: hex32(o.ro_size),
    rw_base: hex32(o.rw_base),
    rw_size: hex32(o.rw_size),
    c_defines: o.c_defines.join(', '),
    c_includes: o.c_includes.join('; '),
    a_defines: o.a_defines.join(', '),
    a_includes: o.a_includes.join('; '),
  };
}

const list = (s: string, sep: RegExp) =>
  s
    .split(sep)
    .map((x) => x.trim())
    .filter(Boolean);

function fromDraft(d: Draft, base: TargetOptions): TargetOptions {
  const { xtal, ...rest } = d;
  return {
    ...base,
    ...rest,
    xtal_hz: Math.round((parseFloat(xtal) || 8) * 1e6),
    ro_base: parseNum(d.ro_base) ?? base.ro_base,
    ro_size: parseNum(d.ro_size) ?? base.ro_size,
    rw_base: parseNum(d.rw_base) ?? base.rw_base,
    rw_size: parseNum(d.rw_size) ?? base.rw_size,
    c_defines: list(d.c_defines, /,/),
    c_includes: list(d.c_includes, /;/),
    a_defines: list(d.a_defines, /,/),
    a_includes: list(d.a_includes, /;/),
  };
}

export function OptionsDialog() {
  const project = useIde((s) => s.project);
  const open = useIde((s) => s.dialog === 'options');
  const [d, setD] = useState<Draft | null>(null);
  const [tab, setTab] = useState('target');
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (open && project) setD(toDraft(project.options));
  }, [open, project]);
  const set =
    <K extends keyof Draft>(k: K) =>
    (v: Draft[K]) =>
      setD((x) => (x ? { ...x, [k]: v } : x));

  return (
    <Modal
      name="options"
      title={`Options for Target '${project?.options.target_name ?? 'Target 1'}'`}
      size="lg"
      footer={
        <>
          <Button onClick={closeDialog}>Cancel</Button>
          <Button
            variant="solid"
            disabled={!d || !project || saving}
            onClick={async () => {
              if (!d || !project) return;
              setSaving(true);
              try {
                await saveOptions(fromDraft(d, project.options));
                closeDialog();
              } catch {
                // saveOptions shows the error
              } finally {
                setSaving(false);
              }
            }}
          >
            OK
          </Button>
        </>
      }
    >
      {!project || !d ? (
        <p className="text-muted">Open a project to edit its target options.</p>
      ) : (
        <Tabs value={tab} onValueChange={(v) => v && setTab(String(v))}>
          <TabsList aria-label="Option pages" className="-mt-1 mb-3.5 gap-0.5">
            <TabsTab value="target" className={SEG_TAB}>
              Target
            </TabsTab>
            <TabsTab value="output" className={SEG_TAB}>
              Output
            </TabsTab>
            <TabsTab value="asm" className={SEG_TAB}>
              Asm
            </TabsTab>
            <TabsTab value="c" className={SEG_TAB}>
              C/C++
            </TabsTab>
            <TabsTab value="debug" className={SEG_TAB}>
              Debug
            </TabsTab>
          </TabsList>
          <TabsPanel value="target" className={formGrid()}>
            <SelectField
              label="Device"
              value={d.device}
              options={DEVICES.some(([v]) => v === d.device) ? DEVICES : [[d.device, d.device], ...DEVICES]}
              onChange={set('device')}
            />
            <TextField label="Xtal (MHz)" value={d.xtal} onChange={set('xtal')} mono hint="Sets the simulator clock used for t1 and SysTick timing." />
            <TextField label="IROM1 Start" value={d.ro_base} onChange={set('ro_base')} mono />
            <TextField label="IROM1 Size" value={d.ro_size} onChange={set('ro_size')} mono />
            <TextField label="IRAM1 Start" value={d.rw_base} onChange={set('rw_base')} mono />
            <TextField label="IRAM1 Size" value={d.rw_size} onChange={set('rw_size')} mono />
          </TabsPanel>
          <TabsPanel value="output" className={formGrid()}>
            <TextField label="Name of Executable" value={d.output_name} onChange={set('output_name')} mono />
            <TextField label="Output Folder" value={d.output_dir} onChange={set('output_dir')} mono />
            <CheckField label="Create HEX File" checked={d.create_hex} onChange={set('create_hex')} />
            <CheckField label="Create BIN File" checked={d.create_bin} onChange={set('create_bin')} />
          </TabsPanel>
          <TabsPanel value="asm" className={formGrid()}>
            <SelectField
              label="Assembler Syntax"
              value={d.asm_syntax}
              options={[
                ['armasm', 'armasm (Arm syntax: AREA, DCD, THUMB)'],
                ['gnuarmclang', 'armclang (GNU syntax)'],
              ]}
              onChange={set('asm_syntax')}
            />
            <TextField label="Define" value={d.a_defines} onChange={set('a_defines')} mono hint="Comma separated." />
            <TextField label="Include Paths" value={d.a_includes} onChange={set('a_includes')} mono hint="Semicolon separated." />
            <TextField label="Misc Controls" value={d.a_misc} onChange={set('a_misc')} mono />
          </TabsPanel>
          <TabsPanel value="c" className={formGrid()}>
            <Note className="col-span-full">C sources build with the Keil toolchain from VS Code. The browser assembles assembly only.</Note>
            <TextField label="Define" value={d.c_defines} onChange={set('c_defines')} mono />
            <TextField label="Include Paths" value={d.c_includes} onChange={set('c_includes')} mono />
            <TextField label="Misc Controls" value={d.c_misc} onChange={set('c_misc')} mono />
          </TabsPanel>
          <TabsPanel value="debug" className={formGrid()}>
            <SelectField
              label="Debugger"
              value={d.debug_mode}
              options={[
                ['simulator', 'Use Simulator (built-in Cortex-M3 + STM32F1)'],
                ['hardware', 'Use ST-Link Debugger (OpenOCD, SWD)'],
              ]}
              onChange={set('debug_mode')}
            />
            <TextField label="OpenOCD Interface" value={d.hw_interface} onChange={set('hw_interface')} mono placeholder="stlink" />
            <TextField label="OpenOCD Target" value={d.hw_target} onChange={set('hw_target')} mono placeholder="stm32f1x" />
          </TabsPanel>
        </Tabs>
      )}
    </Modal>
  );
}
