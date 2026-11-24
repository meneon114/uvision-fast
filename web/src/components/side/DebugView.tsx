import { Input } from '../ui/Input';
import { Empty, VIEW, VIEW_SCROLL } from '../ui/kit';
import { RowName, RowValue, Tree, TreeGroup, TreeItem, TreeItemIndicator, TreeItemLabel } from '../ui/tree';
import { useState } from 'react';
import type { RegisterSnapshot } from '../../host/types';
import { baseName, hex32 } from '../../lib/format';
import { setSearch } from '../../lib/nav';
import { setRegister, setWatchExprs } from '../../state/ide';
import { askText, useIde } from '../../state/store';
import { CmdButton, TbButton } from '../shell/TbButton';
import { Section } from './Section';
import { ViewTitle } from './ViewTitle';

const CORE = ['R0', 'R1', 'R2', 'R3', 'R4', 'R5', 'R6', 'R7', 'R8', 'R9', 'R10', 'R11', 'R12', 'R13 (SP)', 'R14 (LR)', 'R15 (PC)'];
const REG_KEY = (n: string) => n.split(' ')[0];

function Row({ name, value, changed, plain }: { name: string; value: string; changed?: boolean; plain?: boolean }) {
  return (
    <>
      <RowName>{name}</RowName>
      <RowValue plain={plain} changed={changed}>
        {value}
      </RowValue>
    </>
  );
}

function Registers({ r }: { r: RegisterSnapshot }) {
  const ch = new Set(r.changed_regs);
  const flags: [string, boolean][] = [
    ['N', r.n],
    ['Z', r.z],
    ['C', r.c],
    ['V', r.v],
    ['Q', r.q],
    ['T', r.t],
  ];
  const isr = r.xpsr & 0x1ff;
  const edit = async (name: string, current: number) => {
    const v = await askText(`Set ${name}`, 'New value (hex 0x... or decimal)', hex32(current));
    if (v !== null) await setRegister(REG_KEY(name), v);
  };
  return (
    <Tree
      aria-label="Registers"
      mode="none"
      defaultExpanded={['core', 'xpsr', 'internal']}
      onAction={(v) => {
        const i = CORE.indexOf(v);
        if (i >= 0) void edit(v, r.r[i]);
        else if (v === 'xpsr') void edit('xPSR', r.xpsr);
        else if (v === 'msp' || v === 'psp' || v === 'primask' || v === 'control') void edit(v.toUpperCase(), r[v]);
      }}
    >
      <TreeItem value="core">
        <TreeItemLabel>
          <TreeItemIndicator />
          <RowName group>Core</RowName>
        </TreeItemLabel>
        <TreeGroup>
          {CORE.map((n, i) => (
            <TreeItem key={n} value={n}>
              <TreeItemLabel>
                <Row name={n} value={hex32(r.r[i])} changed={ch.has(n) || ch.has(REG_KEY(n))} />
              </TreeItemLabel>
            </TreeItem>
          ))}
          <TreeItem value="xpsr">
            <TreeItemLabel>
              <TreeItemIndicator />
              <Row name="xPSR" value={hex32(r.xpsr)} changed={ch.has('xPSR') || ch.has('XPSR')} />
            </TreeItemLabel>
            <TreeGroup>
              {flags.map(([f, on]) => (
                <TreeItem key={f} value={`flag-${f}`}>
                  <TreeItemLabel>
                    <Row name={f} value={on ? '1' : '0'} changed={ch.has('xPSR') && on} />
                  </TreeItemLabel>
                </TreeItem>
              ))}
              <TreeItem value="flag-isr">
                <TreeItemLabel>
                  <Row name="ISR" value={String(isr)} />
                </TreeItemLabel>
              </TreeItem>
            </TreeGroup>
          </TreeItem>
        </TreeGroup>
      </TreeItem>
      <TreeItem value="banked">
        <TreeItemLabel>
          <TreeItemIndicator />
          <RowName group>Banked</RowName>
        </TreeItemLabel>
        <TreeGroup>
          <TreeItem value="msp">
            <TreeItemLabel>
              <Row name="MSP" value={hex32(r.msp)} changed={ch.has('MSP')} />
            </TreeItemLabel>
          </TreeItem>
          <TreeItem value="psp">
            <TreeItemLabel>
              <Row name="PSP" value={hex32(r.psp)} changed={ch.has('PSP')} />
            </TreeItemLabel>
          </TreeItem>
        </TreeGroup>
      </TreeItem>
      <TreeItem value="system">
        <TreeItemLabel>
          <TreeItemIndicator />
          <RowName group>System</RowName>
        </TreeItemLabel>
        <TreeGroup>
          <TreeItem value="primask">
            <TreeItemLabel>
              <Row name="PRIMASK" value={String(r.primask)} changed={ch.has('PRIMASK')} />
            </TreeItemLabel>
          </TreeItem>
          <TreeItem value="control">
            <TreeItemLabel>
              <Row name="CONTROL" value={String(r.control)} />
            </TreeItemLabel>
          </TreeItem>
        </TreeGroup>
      </TreeItem>
      <TreeItem value="internal">
        <TreeItemLabel>
          <TreeItemIndicator />
          <RowName group>Internal</RowName>
        </TreeItemLabel>
        <TreeGroup>
          <TreeItem value="mode">
            <TreeItemLabel>
              <Row name="Mode" value={r.mode || (isr ? 'Handler' : 'Thread')} plain />
            </TreeItemLabel>
          </TreeItem>
          <TreeItem value="priv">
            <TreeItemLabel>
              <Row name="Privilege" value={r.control & 1 ? 'Unprivileged' : 'Privileged'} plain />
            </TreeItemLabel>
          </TreeItem>
          <TreeItem value="stack">
            <TreeItemLabel>
              <Row name="Stack" value={r.control & 2 ? 'PSP' : 'MSP'} plain />
            </TreeItemLabel>
          </TreeItem>
          <TreeItem value="states">
            <TreeItemLabel>
              <Row name="States" value={String(r.cycles)} />
            </TreeItemLabel>
          </TreeItem>
          <TreeItem value="sec">
            <TreeItemLabel>
              <Row name="Sec" value={r.time_sec.toFixed(8)} />
            </TreeItemLabel>
          </TreeItem>
        </TreeGroup>
      </TreeItem>
    </Tree>
  );
}

function Watch() {
  const exprs = useIde((s) => s.watchExprs);
  const vals = useIde((s) => s.watchVals);
  const session = useIde((s) => s.session);
  const [draft, setDraft] = useState('');
  return (
    <>
      <Tree
        aria-label="Watch 1"
        mode="none"
        onAction={async (v) => {
          const i = Number(v);
          const next = await askText('Edit watch expression', 'Expression (empty removes it)', exprs[i]);
          if (next === null) return;
          const list = [...exprs];
          if (next.trim()) list[i] = next.trim();
          else list.splice(i, 1);
          setWatchExprs(list);
        }}
      >
        {exprs.map((e, i) => {
          const val = vals[e];
          const text = !val ? (session === 'on' ? '...' : '<not in debug>') : 'error' in val ? val.error : val.value_hex;
          return (
            <TreeItem key={`${e}-${i}`} value={String(i)}>
              <TreeItemLabel>
                <RowName mono>{e}</RowName>
                <RowValue error={!!val && 'error' in val} plain={!val} title={val && !('error' in val) ? `${val.value_dec} (${val.kind})` : undefined}>
                  {text}
                </RowValue>
              </TreeItemLabel>
            </TreeItem>
          );
        })}
      </Tree>
      <form
        className="pt-0.5 pr-2.5 pb-1.5 pl-7"
        onSubmit={(ev) => {
          ev.preventDefault();
          if (draft.trim()) setWatchExprs([...exprs, draft.trim()]);
          setDraft('');
        }}
      >
        <Input
          compact
          className="w-full italic"
          aria-label="Add watch expression"
          placeholder="<Enter expression>  R0, *0x20000000, GPIOC.ODR"
          value={draft}
          onValueChange={setDraft}
        />
      </form>
    </>
  );
}

function CallStack() {
  const frames = useIde((s) => s.dbg?.call_stack ?? []);
  if (!frames.length) return <Empty>No frames.</Empty>;
  return (
    <Tree
      aria-label="Call Stack"
      onAction={(v) => {
        const f = frames[Number(v)];
        if (f?.file) setSearch({ file: f.file, line: f.line ?? undefined });
      }}
    >
      {frames.map((f, i) => (
        <TreeItem key={i} value={String(i)}>
          <TreeItemLabel>
            <RowName>{f.symbol || hex32(f.addr)}</RowName>
            <RowValue plain>{f.file ? `${baseName(f.file)}:${f.line ?? '?'}` : hex32(f.addr)}</RowValue>
          </TreeItemLabel>
        </TreeItem>
      ))}
    </Tree>
  );
}

export function DebugView({ title }: { title: string }) {
  const dbg = useIde((s) => s.dbg);
  const session = useIde((s) => s.session);
  return (
    <div className={VIEW}>
      <ViewTitle title={title}>
        <CmdButton id="debug.toggle" pressed={session !== 'off'} />
        <TbButton label="Remove all watch expressions" icon="clear-all" onClick={() => setWatchExprs([])} />
      </ViewTitle>
      <div className={VIEW_SCROLL}>
        <Section title="Registers">
          {dbg ? (
            <Registers r={dbg.registers} />
          ) : (
            <Empty>{session === 'starting' ? 'Starting...' : 'Start a debug session (Ctrl+F5) to see registers.'}</Empty>
          )}
        </Section>
        <Section title="Watch 1">
          <Watch />
        </Section>
        <Section title="Call Stack">{dbg ? <CallStack /> : <Empty>Not debugging.</Empty>}</Section>
      </div>
    </div>
  );
}
