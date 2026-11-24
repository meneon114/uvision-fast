import { Input } from '@ui-sdk/react/input';
import { Toolbar } from '@ui-sdk/react/toolbar';
import { useEffect, useRef, useState } from 'react';
import type { LogicChannel } from '../../host/types';
import { logicAdd, logicRemove } from '../../state/ide';
import { useIde } from '../../state/store';
import { TbButton } from '../shell/TbButton';

const LABEL_W = 120;
const HEADER_H = 20;

interface View {
  zoom: number;
  hoverX: number | null;
}

function cssVar(el: Element, name: string, fallback: string): string {
  return getComputedStyle(el).getPropertyValue(name).trim() || fallback;
}

function valueAt(ch: LogicChannel, cycle: number): number {
  let v = 0;
  for (const s of ch.samples) {
    if (s.cycle <= cycle) v = s.value;
    else break;
  }
  return v;
}

function draw(canvas: HTMLCanvasElement, channels: LogicChannel[], now: number, view: View): string {
  const parent = canvas.parentElement;
  if (!parent) return '';
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(200, parent.clientWidth);
  const h = Math.max(80, parent.clientHeight);
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
  }
  const ctx = canvas.getContext('2d');
  if (!ctx) return '';
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const bg = cssVar(canvas, '--bg', '#ffffff');
  const chrome = cssVar(canvas, '--chrome2', '#f3f3f3');
  const border = cssVar(canvas, '--border', '#e5e5e5');
  const fg = cssVar(canvas, '--fg', '#1f1f1f');
  const muted = cssVar(canvas, '--muted', '#6e6e6e');
  const cursor = cssVar(canvas, '--bp', '#e51400');
  const colors = [
    cssVar(canvas, '--go', '#388a34'),
    cssVar(canvas, '--acc', '#005fb8'),
    cssVar(canvas, '--t-d', '#af5f00'),
    cssVar(canvas, '--t-r', '#267f99'),
    cssVar(canvas, '--t-s', '#a31515'),
  ];
  const mono = '11px "JetBrains Mono", ui-monospace, monospace';

  let maxCycle = Math.max(20, now);
  for (const ch of channels) for (const s of ch.samples) maxCycle = Math.max(maxCycle, s.cycle);

  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, w, h);
  ctx.fillStyle = chrome;
  ctx.fillRect(0, 0, w, HEADER_H);
  ctx.fillRect(0, 0, LABEL_W, h);
  ctx.strokeStyle = border;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(LABEL_W + 0.5, 0);
  ctx.lineTo(LABEL_W + 0.5, h);
  ctx.moveTo(0, HEADER_H + 0.5);
  ctx.lineTo(w, HEADER_H + 0.5);
  ctx.stroke();

  const plotW = Math.max(40, w - LABEL_W - 8);
  const visible = Math.max(10, maxCycle / view.zoom);
  const start = Math.max(0, maxCycle - visible);
  const toX = (c: number) => LABEL_W + ((c - start) / visible) * plotW;

  ctx.font = mono;
  for (let i = 0; i <= 8; i++) {
    const c = start + (visible * i) / 8;
    const x = Math.round(toX(c)) + 0.5;
    ctx.strokeStyle = border;
    ctx.beginPath();
    ctx.moveTo(x, HEADER_H);
    ctx.lineTo(x, h);
    ctx.stroke();
    ctx.fillStyle = muted;
    ctx.fillText(String(Math.round(c)), x + 3, 14);
  }

  const rowH = Math.max(26, Math.min(44, (h - HEADER_H) / Math.max(1, channels.length)));
  channels.forEach((ch, idx) => {
    const top = HEADER_H + idx * rowH;
    const bot = top + rowH;
    const color = colors[idx % colors.length];
    ctx.strokeStyle = border;
    ctx.beginPath();
    ctx.moveTo(0, Math.round(bot) + 0.5);
    ctx.lineTo(w, Math.round(bot) + 0.5);
    ctx.stroke();
    ctx.fillStyle = fg;
    ctx.font = `600 ${mono}`;
    ctx.fillText(ch.signal.slice(0, 15), 8, top + rowH / 2 + 4);
    if (!ch.samples.length) return;

    const bit = ch.samples.every((s) => s.value === 0 || s.value === 1);
    const hi = top + 6;
    const lo = bot - 6;
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.font = mono;
    if (bit) {
      ctx.beginPath();
      ch.samples.forEach((s, i) => {
        const x = Math.max(LABEL_W, toX(s.cycle));
        const y = s.value ? hi : lo;
        if (i === 0) ctx.moveTo(x, y);
        else {
          ctx.lineTo(x, ch.samples[i - 1].value ? hi : lo);
          ctx.lineTo(x, y);
        }
        const next = i + 1 < ch.samples.length ? ch.samples[i + 1].cycle : maxCycle;
        ctx.lineTo(Math.min(w, toX(next)), y);
      });
      ctx.stroke();
    } else {
      ch.samples.forEach((s, i) => {
        const x1 = Math.max(LABEL_W, toX(s.cycle));
        const next = i + 1 < ch.samples.length ? ch.samples[i + 1].cycle : maxCycle;
        const x2 = Math.min(w, toX(next));
        if (x2 <= LABEL_W) return;
        ctx.strokeRect(x1, hi, Math.max(2, x2 - x1), lo - hi);
        if (x2 - x1 > 50) {
          ctx.fillStyle = color;
          ctx.fillText(`0x${s.value.toString(16).toUpperCase()}`, x1 + 4, top + rowH / 2 + 4);
        }
      });
    }
    ctx.lineWidth = 1;
  });

  if (view.hoverX === null || view.hoverX < LABEL_W) return '';
  ctx.strokeStyle = cursor;
  ctx.beginPath();
  ctx.moveTo(view.hoverX + 0.5, HEADER_H);
  ctx.lineTo(view.hoverX + 0.5, h);
  ctx.stroke();
  const cyc = start + ((view.hoverX - LABEL_W) / plotW) * visible;
  return `Cycle ${Math.round(cyc)}   ${channels.map((c) => `${c.signal}=0x${valueAt(c, cyc).toString(16).toUpperCase()}`).join('   ')}`;
}

export function LogicAnalyzer() {
  const channels = useIde((s) => s.logic);
  const now = useIde((s) => s.dbg?.registers.cycles ?? 0);
  const theme = useIde((s) => s.theme);
  const session = useIde((s) => s.session);
  const canvas = useRef<HTMLCanvasElement>(null);
  const view = useRef<View>({ zoom: 1, hoverX: null });
  const [info, setInfo] = useState('');
  const [signal, setSignal] = useState('');

  const redraw = () => {
    if (canvas.current) setInfo(draw(canvas.current, channels, now, view.current));
  };

  useEffect(redraw, [channels, now, theme]);
  useEffect(() => {
    const el = canvas.current?.parentElement;
    if (!el) return;
    const ro = new ResizeObserver(() => redraw());
    ro.observe(el);
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    mq.addEventListener('change', redraw);
    return () => {
      ro.disconnect();
      mq.removeEventListener('change', redraw);
    };
  });

  const zoom = (f: number | null) => {
    view.current.zoom = f === null ? 1 : Math.min(50, Math.max(1, view.current.zoom * f));
    redraw();
  };

  return (
    <div className="logic">
      <div className="logic-bar">
        <Toolbar aria-label="Logic Analyzer" className="logic-tools">
          <TbButton label="Zoom In" icon="zoom-in" onClick={() => zoom(1.5)} />
          <TbButton label="Zoom Out" icon="zoom-out" onClick={() => zoom(1 / 1.5)} />
          <TbButton label="Zoom to Fit" icon="screen-full" onClick={() => zoom(null)} />
        </Toolbar>
        <form
          className="logic-add"
          onSubmit={(e) => {
            e.preventDefault();
            if (signal.trim()) void logicAdd(signal.trim());
            setSignal('');
          }}
        >
          <Input
            className="mono"
            aria-label="Add signal"
            placeholder="Add signal: GPIOC.ODR, R0, TIM2.CNT"
            value={signal}
            onValueChange={setSignal}
            disabled={session !== 'on'}
          />
        </form>
        <ul className="logic-sigs" aria-label="Signals">
          {channels.map((c) => (
            <li key={c.signal}>
              <span className="mono">{c.signal}</span>
              <button type="button" className="icon-btn" aria-label={`Remove ${c.signal}`} onClick={() => void logicRemove(c.signal)}>
                <i className="codicon codicon-close" aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
        <span className="grow" />
        <span className="logic-info mono" aria-live="polite">
          {info}
        </span>
      </div>
      <div className="logic-plot">
        {session !== 'on' && !channels.length ? <div className="out muted">Start a debug session to trace signals.</div> : null}
        <canvas
          ref={canvas}
          role="img"
          aria-label={`Logic analyzer trace of ${channels.map((c) => c.signal).join(', ') || 'no signals'}`}
          onPointerMove={(e) => {
            view.current.hoverX = e.clientX - e.currentTarget.getBoundingClientRect().left;
            redraw();
          }}
          onPointerLeave={() => {
            view.current.hoverX = null;
            redraw();
          }}
          onWheel={(e) => zoom(e.deltaY < 0 ? 1.25 : 0.8)}
        />
      </div>
    </div>
  );
}
