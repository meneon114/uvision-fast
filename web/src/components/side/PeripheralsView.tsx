import { Button } from '@ui-sdk/react/button';
import { Checkbox, CheckboxIndicator } from '@ui-sdk/react/checkbox';
import { Select, SelectContent, SelectItem, SelectItemIndicator, SelectTrigger, SelectValue } from '@ui-sdk/react/select';
import { Switch, SwitchThumb } from '@ui-sdk/react/switch';
import { Toggle } from '@ui-sdk/react/toggle';
import { Tree, TreeGroup, TreeItem, TreeItemIndicator, TreeItemLabel } from '@ui-sdk/react/tree';
import type { GpioPort, PeripheralSnapshot } from '../../host/types';
import { hex32 } from '../../lib/format';
import { KEYPAD_KEYS, ledLit, pressButton, pressKey, setBoard, setPin } from '../../state/board';
import { useIde, type BoardButton, type BoardLed } from '../../state/store';
import { Section } from './Section';
import { ViewTitle } from './ViewTitle';
import { CmdButton } from '../shell/TbButton';

const PINS = [...Array.from({ length: 16 }, (_, i) => `PA${i}`), ...Array.from({ length: 16 }, (_, i) => `PB${i}`), 'PC13', 'PC14', 'PC15'];

function pinMode(port: GpioPort, pin: number): { mode: 'in' | 'out' | 'af' | 'an'; text: string } {
  const reg = pin < 8 ? port.crl : port.crh;
  const nib = (reg >>> ((pin % 8) * 4)) & 0xf;
  const mode = nib & 3;
  const cnf = nib >> 2;
  if (mode === 0)
    return cnf === 0 ? { mode: 'an', text: 'Analog' } : cnf === 1 ? { mode: 'in', text: 'Input floating' } : { mode: 'in', text: 'Input pull-up/down' };
  const speed = ['', '10 MHz', '2 MHz', '50 MHz'][mode];
  return cnf < 2
    ? { mode: 'out', text: `Output ${cnf ? 'open-drain' : 'push-pull'} ${speed}` }
    : { mode: 'af', text: `Alternate ${cnf === 3 ? 'open-drain' : 'push-pull'} ${speed}` };
}

function GpioPins({ port, live }: { port: GpioPort; live: boolean }) {
  const letter = port.name.slice(-1);
  return (
    <div className="gpio">
      <div className="gpio-regs mono">
        <span>CRL {hex32(port.crl)}</span>
        <span>CRH {hex32(port.crh)}</span>
        <span>IDR {hex32(port.idr)}</span>
        <span>ODR {hex32(port.odr)}</span>
      </div>
      <div className="pin-grid" role="group" aria-label={`${port.name} pins`}>
        {Array.from({ length: 16 }, (_, k) => 15 - k).map((pin) => {
          const m = pinMode(port, pin);
          const high = m.mode === 'out' || m.mode === 'af' ? ((port.odr >> pin) & 1) === 1 : ((port.idr >> pin) & 1) === 1;
          const name = `P${letter}${pin}`;
          return (
            <Toggle
              key={pin}
              className="pin"
              data-mode={m.mode}
              pressed={high}
              disabled={!live || m.mode !== 'in'}
              aria-label={`${name}, ${m.text}, ${high ? 'high' : 'low'}`}
              title={`${name}: ${m.text}${m.mode === 'in' ? ' (click to drive)' : ''}`}
              onPressedChange={(v: boolean) => void setPin(name, v)}
            >
              <span className="pin-n">{pin}</span>
              <span className="pin-l">{high ? '1' : '0'}</span>
            </Toggle>
          );
        })}
      </div>
    </div>
  );
}

function PinSelect({ value, onChange, label }: { value: string; onChange: (v: string) => void; label: string }) {
  return (
    <Select value={value} onValueChange={(v) => v && onChange(String(v))}>
      <SelectTrigger aria-label={label} className="pin-select">
        <SelectValue />
      </SelectTrigger>
      <SelectContent className="pin-select-list">
        {PINS.map((p) => (
          <SelectItem key={p} value={p}>
            {p}
            <SelectItemIndicator />
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function LabBoard({ p }: { p: PeripheralSnapshot | null }) {
  const board = useIde((s) => s.board);
  const down = useIde((s) => s.buttonsDown);
  const keyDown = useIde((s) => s.keyDown);
  const live = useIde((s) => s.session === 'on');
  const pc13 = p ? ledLit(p, { id: 'pc13', pin: 'PC13', activeLow: true }) : false;

  const setLed = (id: string, patch: Partial<BoardLed>) => setBoard({ leds: board.leds.map((l) => (l.id === id ? { ...l, ...patch } : l)) });
  const setBtn = (id: string, patch: Partial<BoardButton>) => setBoard({ buttons: board.buttons.map((b) => (b.id === id ? { ...b, ...patch } : b)) });

  const hold = (id: string) => ({
    onPointerDown: (e: React.PointerEvent) => {
      try {
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      } catch {
        // synthetic or already released pointers cannot be captured
      }
      void pressButton(id, true);
    },
    onPointerUp: () => void pressButton(id, false),
    onPointerCancel: () => void pressButton(id, false),
    onKeyDown: (e: React.KeyboardEvent) => {
      if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) void pressButton(id, true);
    },
    onKeyUp: (e: React.KeyboardEvent) => {
      if (e.key === ' ' || e.key === 'Enter') void pressButton(id, false);
    },
  });

  return (
    <div className="board">
      {!live ? <p className="empty">Start debugging to drive inputs. Bindings apply to the next session.</p> : null}
      <div className="board-row">
        <div className="led-cell">
          <span className="led" data-on={pc13 || undefined} data-color="green" role="img" aria-label={`PC13 on-board LED ${pc13 ? 'on' : 'off'}`} />
          <span className="led-name">PC13</span>
          <span className="muted small">on-board, active low</span>
        </div>
      </div>
      <h3 className="board-h">LEDs</h3>
      {board.leds.map((l, i) => {
        const on = p ? ledLit(p, l) : false;
        return (
          <div className="board-row" key={l.id}>
            <span
              className="led"
              data-on={on || undefined}
              data-color={['red', 'yellow', 'green', 'blue'][i % 4]}
              role="img"
              aria-label={`LED ${i + 1} on ${l.pin}: ${on ? 'on' : 'off'}`}
            />
            <PinSelect label={`LED ${i + 1} pin`} value={l.pin} onChange={(v) => setLed(l.id, { pin: v })} />
            <label className="check">
              <Checkbox
                checked={l.activeLow}
                onCheckedChange={(c) => {
                  const v = c === true;
                  setLed(l.id, { activeLow: v });
                }}
              >
                <CheckboxIndicator />
              </Checkbox>
              Active low
            </label>
          </div>
        );
      })}
      <h3 className="board-h">Buttons</h3>
      {board.buttons.map((b, i) => (
        <div className="board-row" key={b.id}>
          <Button
            className="push"
            data-down={down[b.id] || undefined}
            disabled={!live}
            aria-label={`Button ${i + 1} on ${b.pin}${b.pin === 'PA0' ? ' (EXTI0)' : ''}, hold to press`}
            {...hold(b.id)}
          >
            {b.pin === 'PA0' ? 'EXTI0' : `BTN${i + 1}`}
          </Button>
          <PinSelect label={`Button ${i + 1} pin`} value={b.pin} onChange={(v) => setBtn(b.id, { pin: v })} />
          <label className="check">
            <Checkbox
              checked={b.activeLow}
              onCheckedChange={(c) => {
                const v = c === true;
                setBtn(b.id, { activeLow: v });
              }}
            >
              <CheckboxIndicator />
            </Checkbox>
            Active low
          </label>
        </div>
      ))}
      <h3 className="board-h">
        <label className="check">
          4x3 Keypad
          <span className="pins-note">PA0-3 rows, PA4-6 cols</span>
          <Switch checked={board.keypad} onCheckedChange={(c) => setBoard({ keypad: c === true })}>
            <SwitchThumb />
          </Switch>
        </label>
      </h3>
      {board.keypad ? (
        <div className="keypad" role="group" aria-label="Keypad">
          {KEYPAD_KEYS.map((k) => (
            <Button
              key={k}
              className="key"
              data-down={keyDown === k || undefined}
              disabled={!live}
              aria-label={`Key ${k}`}
              onPointerDown={(e: React.PointerEvent) => {
                (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
                void pressKey(k);
              }}
              onPointerUp={() => void pressKey(null)}
              onPointerCancel={() => void pressKey(null)}
              onKeyDown={(e: React.KeyboardEvent) => {
                if ((e.key === ' ' || e.key === 'Enter') && !e.repeat) void pressKey(k);
              }}
              onKeyUp={() => void pressKey(null)}
            >
              {k}
            </Button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function regsOf(p: PeripheralSnapshot): { id: string; name: string; regs: [string, number][] }[] {
  const out: { id: string; name: string; regs: [string, number][] }[] = [];
  out.push({ id: 'rcc', name: 'RCC', regs: Object.entries(p.rcc).map(([k, v]) => [k.toUpperCase(), v]) });
  for (const g of p.gpios)
    out.push({
      id: g.name,
      name: g.name,
      regs: [
        ['CRL', g.crl],
        ['CRH', g.crh],
        ['IDR', g.idr],
        ['ODR', g.odr],
        ['LCKR', g.lckr],
      ],
    });
  for (const t of p.timers)
    out.push({
      id: t.name,
      name: `${t.name} (IRQ ${t.irq_num})`,
      regs: [
        ['CR1', t.cr1],
        ['CR2', t.cr2],
        ['SMCR', t.smcr],
        ['DIER', t.dier],
        ['SR', t.sr],
        ['CCMR1', t.ccmr1],
        ['CCMR2', t.ccmr2],
        ['CCER', t.ccer],
        ['CNT', t.cnt],
        ['PSC', t.psc],
        ['ARR', t.arr],
        ['RCR', t.rcr],
        ...t.ccr.map((c, i) => [`CCR${i + 1}`, c] as [string, number]),
      ],
    });
  for (const u of p.usarts)
    out.push({
      id: u.name,
      name: u.name,
      regs: [
        ['SR', u.sr],
        ['BRR', u.brr],
        ['CR1', u.cr1],
        ['CR2', u.cr2],
        ['CR3', u.cr3],
      ],
    });
  out.push({
    id: 'systick',
    name: 'SysTick',
    regs: [
      ['CTRL', p.systick.ctrl],
      ['LOAD', p.systick.load],
      ['VAL', p.systick.val],
      ['CALIB', p.systick.calib],
    ],
  });
  out.push({ id: 'scb', name: 'SCB', regs: [['VTOR', p.vtor]] });
  return out;
}

function SystemViewer({ p }: { p: PeripheralSnapshot }) {
  return (
    <Tree aria-label="System Viewer" className="tree regs" mode="none" defaultExpanded={['GPIOC', 'TIM2']}>
      {regsOf(p).map((per) => (
        <TreeItem key={per.id} value={per.id}>
          <TreeItemLabel>
            <TreeItemIndicator />
            <span className="rn grp">{per.name}</span>
          </TreeItemLabel>
          <TreeGroup>
            {per.regs.map(([n, v]) => (
              <TreeItem key={n} value={`${per.id}.${n}`}>
                <TreeItemLabel>
                  <span className="rn">{n}</span>
                  <span className="rv">{hex32(v)}</span>
                </TreeItemLabel>
              </TreeItem>
            ))}
          </TreeGroup>
        </TreeItem>
      ))}
    </Tree>
  );
}

export function PeripheralsView({ title }: { title: string }) {
  const p = useIde((s) => s.periph);
  const live = useIde((s) => s.session === 'on');
  const ports = p?.gpios.filter((g) => ['GPIOA', 'GPIOB', 'GPIOC'].includes(g.name)) ?? [];
  return (
    <div className="view">
      <ViewTitle title={title}>
        <CmdButton id="debug.toggle" pressed={live} />
      </ViewTitle>
      <div className="view-scroll">
        <Section title="Lab I/O Board">
          <LabBoard p={p} />
        </Section>
        <Section title="General Purpose I/O">
          {ports.length ? (
            ports.map((g) => (
              <div key={g.name} className="gpio-port">
                <h3 className="board-h">{g.name}</h3>
                <GpioPins port={g} live={live} />
              </div>
            ))
          ) : (
            <p className="empty">Start a debug session to see pin states.</p>
          )}
        </Section>
        <Section title="System Viewer" defaultOpen={false}>
          {p ? <SystemViewer p={p} /> : <p className="empty">Not debugging.</p>}
        </Section>
      </div>
    </div>
  );
}
