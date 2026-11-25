import { Button as BaseButton } from '@ui-sdk/react/button';
import { Switch, SwitchThumb } from '@ui-sdk/react/switch';
import { Toggle } from '@ui-sdk/react/toggle';
import { Button } from '../ui/Button';
import { Check } from '../ui/Check';
import { CAPS, Empty, VIEW, VIEW_SCROLL } from '../ui/kit';
import { Select, SelectContent, SelectItem, SelectItemIndicator, SelectTrigger, SelectValue } from '../ui/select';
import { RowName, RowValue, Tree, TreeGroup, TreeItem, TreeItemIndicator, TreeItemLabel } from '../ui/tree';
import type { GpioPort, PeripheralSnapshot } from '../../host/types';
import { hex32 } from '../../lib/format';
import { KEYPAD_KEYS, ledLit, pressButton, pressKey, setBoard, setPin } from '../../state/board';
import { useIde, type BoardButton, type BoardLed } from '../../state/store';
import { Section } from './Section';
import { ViewTitle } from './ViewTitle';
import { CmdButton } from '../shell/TbButton';

const BOARD_H = `mt-1.5 ${CAPS}`;
const BOARD_ROW = 'flex items-center gap-2 min-h-6.5 flex-wrap';

// The four board LED colours, read by the lit and unlit LED styles through --led.
const LED_COLOR = ['[--led:var(--led-red)]', '[--led:var(--led-yellow)]', '[--led:var(--led-green)]', '[--led:var(--led-blue)]'];

function Led({ on, color, label }: { on: boolean; color: string; label: string }) {
  return (
    <span
      role="img"
      aria-label={label}
      data-on={on || undefined}
      className={`size-3.5 rounded-full flex-none ${color} bg-[color-mix(in_srgb,var(--led)_22%,var(--chrome2))] shadow-[inset_0_0_0_1px_color-mix(in_srgb,var(--led)_45%,var(--border2))] transition-[background-color] duration-60 ease-linear data-on:bg-(--led) data-on:shadow-[0_0_8px_1px_color-mix(in_srgb,var(--led)_70%,transparent),inset_0_0_0_1px_color-mix(in_srgb,var(--led)_70%,black)]`}
    />
  );
}

const PIN =
  'h-8.5 flex flex-col items-center justify-center gap-0 p-0 border border-border2 rounded-xs bg-bg text-fg2 font-mono cursor-pointer disabled:cursor-default not-aria-pressed:data-[mode=out]:bg-chrome2 data-[mode=af]:border-dashed data-[mode=an]:opacity-55 aria-pressed:bg-[color-mix(in_srgb,var(--go)_18%,var(--bg))] aria-pressed:border-go aria-pressed:text-fg';

const KEY =
  'h-9 px-3 border border-b-2 border-border2 rounded-sm bg-bg text-fg font-mono text-md font-semibold cursor-pointer select-none touch-none transition-[background-color,translate] duration-120 ease-snap not-disabled:hover:bg-hover data-down:bg-sel data-down:border-acc data-down:border-b data-down:translate-y-px disabled:opacity-50 disabled:cursor-default';

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
    <div>
      <div className="flex flex-wrap gap-x-3 gap-y-0.5 mt-0.5 mb-1.5 font-mono text-xs text-muted">
        <span>CRL {hex32(port.crl)}</span>
        <span>CRH {hex32(port.crh)}</span>
        <span>IDR {hex32(port.idr)}</span>
        <span>ODR {hex32(port.odr)}</span>
      </div>
      <div className="grid grid-cols-8 gap-0.75" role="group" aria-label={`${port.name} pins`}>
        {Array.from({ length: 16 }, (_, k) => 15 - k).map((pin) => {
          const m = pinMode(port, pin);
          const high = m.mode === 'out' || m.mode === 'af' ? ((port.odr >> pin) & 1) === 1 : ((port.idr >> pin) & 1) === 1;
          const name = `P${letter}${pin}`;
          return (
            <Toggle
              key={pin}
              className={PIN}
              data-mode={m.mode}
              pressed={high}
              disabled={!live || m.mode !== 'in'}
              aria-label={`${name}, ${m.text}, ${high ? 'high' : 'low'}`}
              title={`${name}: ${m.text}${m.mode === 'in' ? ' (click to drive)' : ''}`}
              onPressedChange={(v: boolean) => void setPin(name, v)}
            >
              <span className="text-3xs text-muted">{pin}</span>
              <span className="text-sm font-semibold">{high ? '1' : '0'}</span>
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
      <SelectTrigger
        aria-label={label}
        className="h-6 min-w-19 inline-flex items-center justify-between gap-1.5 px-1.5 border border-border2 rounded-sm bg-bg text-fg font-mono text-sm cursor-pointer"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent className="max-h-65 overflow-auto min-w-25!">
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
    <div className="flex flex-col gap-1.5 pt-1 pr-3 pb-2.5 pl-5">
      {!live ? <Empty className="p-0 pb-1">Start debugging to drive inputs. Bindings apply to the next session.</Empty> : null}
      <div className={BOARD_ROW}>
        <div className={BOARD_ROW}>
          <Led on={pc13} color={LED_COLOR[2]} label={`PC13 on-board LED ${pc13 ? 'on' : 'off'}`} />
          <span className="font-mono text-sm text-fg">PC13</span>
          <span className="text-muted text-xs">on-board, active low</span>
        </div>
      </div>
      <h3 className={BOARD_H}>LEDs</h3>
      {board.leds.map((l, i) => {
        const on = p ? ledLit(p, l) : false;
        return (
          <div className={BOARD_ROW} key={l.id}>
            <Led on={on} color={LED_COLOR[i % 4]} label={`LED ${i + 1} on ${l.pin}: ${on ? 'on' : 'off'}`} />
            <PinSelect label={`LED ${i + 1} pin`} value={l.pin} onChange={(v) => setLed(l.id, { pin: v })} />
            <Check checked={l.activeLow} onChange={(v) => setLed(l.id, { activeLow: v })}>
              Active low
            </Check>
          </div>
        );
      })}
      <h3 className={BOARD_H}>Buttons</h3>
      {board.buttons.map((b, i) => (
        <div className={BOARD_ROW} key={b.id}>
          <Button
            className="min-w-23 justify-center data-down:bg-sel data-down:border-acc data-down:text-acc data-down:translate-y-px"
            data-down={down[b.id] || undefined}
            disabled={!live}
            aria-label={`Button ${i + 1} on ${b.pin}${b.pin === 'PA0' ? ' (EXTI0)' : ''}, hold to press`}
            {...hold(b.id)}
          >
            {b.pin === 'PA0' ? 'EXTI0' : `BTN${i + 1}`}
          </Button>
          <PinSelect label={`Button ${i + 1} pin`} value={b.pin} onChange={(v) => setBtn(b.id, { pin: v })} />
          <Check checked={b.activeLow} onChange={(v) => setBtn(b.id, { activeLow: v })}>
            Active low
          </Check>
        </div>
      ))}
      <h3 className={BOARD_H}>
        <label className="flex items-center w-full gap-2 text-2xs cursor-pointer">
          4x3 Keypad
          <span className="flex-1 font-normal normal-case tracking-normal">PA0-3 rows, PA4-6 cols</span>
          <Switch checked={board.keypad} onCheckedChange={(c) => setBoard({ keypad: c === true })} className="data-checked:bg-acc">
            <SwitchThumb />
          </Switch>
        </label>
      </h3>
      {board.keypad ? (
        <div className="grid grid-cols-[repeat(3,44px)] gap-1.5 mt-1" role="group" aria-label="Keypad">
          {KEYPAD_KEYS.map((k) => (
            <BaseButton
              key={k}
              className={KEY}
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
              onKeyUp={(e: React.KeyboardEvent) => {
                if (e.key === ' ' || e.key === 'Enter') void pressKey(null);
              }}
            >
              {k}
            </BaseButton>
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
    <Tree aria-label="System Viewer" mode="none" defaultExpanded={['GPIOC', 'TIM2']}>
      {regsOf(p).map((per) => (
        <TreeItem key={per.id} value={per.id}>
          <TreeItemLabel>
            <TreeItemIndicator />
            <RowName group>{per.name}</RowName>
          </TreeItemLabel>
          <TreeGroup>
            {per.regs.map(([n, v]) => (
              <TreeItem key={n} value={`${per.id}.${n}`}>
                <TreeItemLabel>
                  <RowName>{n}</RowName>
                  <RowValue>{hex32(v)}</RowValue>
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
    <div className={VIEW}>
      <ViewTitle title={title}>
        <CmdButton id="debug.toggle" pressed={live} />
      </ViewTitle>
      <div className={VIEW_SCROLL}>
        <Section title="Lab I/O Board">
          <LabBoard p={p} />
        </Section>
        <Section title="General Purpose I/O">
          {ports.length ? (
            ports.map((g) => (
              <div key={g.name} className="pt-0.5 pr-3 pb-2 pl-5">
                <h3 className={BOARD_H}>{g.name}</h3>
                <GpioPins port={g} live={live} />
              </div>
            ))
          ) : (
            <Empty>Start a debug session to see pin states.</Empty>
          )}
        </Section>
        <Section title="System Viewer" defaultOpen={false}>
          {p ? <SystemViewer p={p} /> : <Empty>Not debugging.</Empty>}
        </Section>
      </div>
    </div>
  );
}
