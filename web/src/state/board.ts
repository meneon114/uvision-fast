import { host } from '../host/client';
import type { PeripheralSnapshot } from '../host/types';
import { errMsg } from '../lib/format';
import { ide, type BoardLed } from './store';

// The lab I/O board drives external pin levels into the simulator through
// debug.setGpioPin. It never reads back from the core except via the
// peripheral snapshot, so it works the same with any core that models GPIO IDR.

export interface PinRef {
  port: string;
  pin: number;
}

export function parsePin(spec: string): PinRef | null {
  const m = /^P([A-E])(\d{1,2})$/i.exec(spec.trim());
  if (!m) return null;
  const pin = Number(m[2]);
  if (pin > 15) return null;
  return { port: `GPIO${m[1].toUpperCase()}`, pin };
}

export const KEYPAD_KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '*', '0', '#'] as const;
export const KEYPAD_ROWS = ['PA0', 'PA1', 'PA2', 'PA3'];
export const KEYPAD_COLS = ['PA4', 'PA5', 'PA6'];

const driven = new Map<string, boolean>();

async function drive(spec: string, high: boolean, force = false): Promise<void> {
  const ref = parsePin(spec);
  if (!ref || ide.get().session !== 'on') return;
  const key = `${ref.port}.${ref.pin}`;
  if (!force && driven.get(key) === high) return;
  driven.set(key, high);
  try {
    const snap = await host().rpc<PeripheralSnapshot>('debug.setGpioPin', { port: ref.port, pin: ref.pin, high });
    ide.set({ periph: snap });
  } catch (e) {
    driven.delete(key);
    ide.set({ status: `Pin ${spec}: ${errMsg(e)}` });
  }
}

export function setPin(spec: string, high: boolean): Promise<void> {
  return drive(spec, high, true);
}

export function pinLevel(p: PeripheralSnapshot | null, spec: string, reg: 'idr' | 'odr' = 'idr'): boolean | null {
  const ref = parsePin(spec);
  if (!p || !ref) return null;
  const port = p.gpios.find((g) => g.name === ref.port);
  if (!port) return null;
  return ((port[reg] >> ref.pin) & 1) === 1;
}

export function ledLit(p: PeripheralSnapshot | null, led: BoardLed): boolean {
  const high = pinLevel(p, led.pin, 'odr');
  if (high === null) return false;
  return led.activeLow ? !high : high;
}

// Released buttons sit at their idle level; keypad columns idle high (the module's pull-ups).
export async function applyBoardInputs(): Promise<void> {
  driven.clear();
  linked = null;
  const st = ide.get();
  for (const b of st.board.buttons) await drive(b.pin, b.activeLow, true);
  if (st.board.keypad) for (const c of KEYPAD_COLS) await drive(c, true, true);
}

export async function pressButton(id: string, down: boolean): Promise<void> {
  const b = ide.get().board.buttons.find((x) => x.id === id);
  if (!b) return;
  ide.set((s) => ({ buttonsDown: { ...s.buttonsDown, [id]: down } }));
  await drive(b.pin, down ? !b.activeLow : b.activeLow, true);
}

// The pressed key's switch joins its row pin to its column pin. The core models
// the contact (debug.connectPins), so a scan loop sees it on every instruction
// rather than only between run chunks.
let linked: [string, string] | null = null;

async function link(pair: [string, string], connected: boolean): Promise<void> {
  try {
    const snap = await host().rpc<PeripheralSnapshot>('debug.connectPins', { a: pair[0], b: pair[1], connected });
    ide.set({ periph: snap });
  } catch (e) {
    ide.set({ status: `Keypad: ${errMsg(e)}` });
  }
}

function keyPins(key: string | null): [string, string] | null {
  const idx = key ? KEYPAD_KEYS.indexOf(key as (typeof KEYPAD_KEYS)[number]) : -1;
  return idx >= 0 ? [KEYPAD_ROWS[Math.floor(idx / 3)], KEYPAD_COLS[idx % 3]] : null;
}

export async function pressKey(key: string | null): Promise<void> {
  ide.set({ keyDown: key });
  const st = ide.get();
  if (st.session !== 'on') return;
  const next = st.board.keypad ? keyPins(key) : null;
  if (linked && linked.join() !== next?.join()) await link(linked, false);
  if (next && linked?.join() !== next.join()) await link(next, true);
  linked = next;
}

export function setBoard(patch: Partial<ReturnType<typeof ide.get>['board']>): void {
  ide.set((s) => ({ board: { ...s.board, ...patch } }));
  if (ide.get().session === 'on') void applyBoardInputs();
}
