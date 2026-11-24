export interface LogicSample {
  cycle: number;
  value: number;
}

export interface LogicChannel {
  signal: string;
  samples: LogicSample[];
}

const COLORS = ['#008000', '#0000cc', '#aa00aa', '#cc6600', '#008080', '#990000'];

export class LogicAnalyzerCanvas {
  private channels: LogicChannel[] = [];
  private zoom = 1.0;
  private panOffsetCycles = 0;
  private hoverX: number | null = null;
  private maxCycle = 100;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly onHoverInfo: (info: string) => void
  ) {
    canvas.addEventListener('mousemove', (e) => {
      const rect = canvas.getBoundingClientRect();
      this.hoverX = e.clientX - rect.left;
      this.draw();
    });
    canvas.addEventListener('mouseleave', () => {
      this.hoverX = null;
      this.onHoverInfo('');
      this.draw();
    });
    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      if (e.deltaY < 0) {
        this.zoom = Math.min(20, this.zoom * 1.25);
      } else {
        this.zoom = Math.max(0.25, this.zoom / 1.25);
      }
      this.draw();
    });
  }

  public setChannels(channels: LogicChannel[], currentCycle: number): void {
    this.channels = channels;
    let mx = Math.max(20, currentCycle);
    for (const ch of channels) {
      for (const s of ch.samples) {
        if (s.cycle > mx) mx = s.cycle;
      }
    }
    this.maxCycle = mx;
    this.draw();
  }

  public zoomIn(): void {
    this.zoom = Math.min(20, this.zoom * 1.3);
    this.draw();
  }

  public zoomOut(): void {
    this.zoom = Math.max(0.25, this.zoom / 1.3);
    this.draw();
  }

  public zoomFit(): void {
    this.zoom = 1.0;
    this.panOffsetCycles = 0;
    this.draw();
  }

  public draw(): void {
    const parent = this.canvas.parentElement;
    if (!parent) return;
    const w = Math.max(300, parent.clientWidth);
    const h = Math.max(100, parent.clientHeight);
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }

    const ctx = this.canvas.getContext('2d');
    if (!ctx) return;

    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, w, h);

    const labelW = 115;
    const plotW = Math.max(50, w - labelW - 10);
    const headerH = 20;

    // Header time grid
    ctx.fillStyle = '#f0f0f0';
    ctx.fillRect(0, 0, w, headerH);
    ctx.fillRect(0, 0, labelW, h);
    ctx.strokeStyle = '#a0a0a0';
    ctx.beginPath();
    ctx.moveTo(labelW, 0);
    ctx.lineTo(labelW, h);
    ctx.moveTo(0, headerH);
    ctx.lineTo(w, headerH);
    ctx.stroke();

    const visibleCycles = Math.max(10, this.maxCycle / this.zoom);
    const startCycle = Math.max(0, this.panOffsetCycles);
    const toX = (cyc: number) => labelW + ((cyc - startCycle) / visibleCycles) * plotW;

    // Vertical gridlines
    ctx.font = '10px Consolas, monospace';
    const ticks = 8;
    for (let i = 0; i <= ticks; i++) {
      const cyc = startCycle + (visibleCycles * i) / ticks;
      const x = toX(cyc);
      ctx.strokeStyle = '#ececec';
      ctx.beginPath();
      ctx.moveTo(x, headerH);
      ctx.lineTo(x, h);
      ctx.stroke();
      ctx.fillStyle = '#444444';
      ctx.fillText(`${Math.round(cyc)}`, x + 2, 14);
    }

    const rowH = Math.max(28, Math.min(44, (h - headerH) / Math.max(1, this.channels.length)));

    this.channels.forEach((ch, idx) => {
      const yTop = headerH + idx * rowH;
      const yBot = yTop + rowH;
      const color = COLORS[idx % COLORS.length];

      ctx.strokeStyle = '#dcdcdc';
      ctx.beginPath();
      ctx.moveTo(0, yBot);
      ctx.lineTo(w, yBot);
      ctx.stroke();

      ctx.fillStyle = '#111111';
      ctx.font = 'bold 11px Consolas, monospace';
      ctx.fillText(ch.signal.slice(0, 14), 6, yTop + rowH / 2 + 4);

      if (ch.samples.length === 0) return;

      const isSingleBit = ch.samples.every((s) => s.value === 0 || s.value === 1);
      const yHigh = yTop + 6;
      const yLow = yBot - 6;

      ctx.strokeStyle = color;
      ctx.lineWidth = 1.5;

      if (isSingleBit) {
        ctx.beginPath();
        for (let i = 0; i < ch.samples.length; i++) {
          const s = ch.samples[i];
          const x = Math.max(labelW, toX(s.cycle));
          const y = s.value ? yHigh : yLow;
          if (i === 0) {
            ctx.moveTo(x, y);
          } else {
            const prevY = ch.samples[i - 1].value ? yHigh : yLow;
            ctx.lineTo(x, prevY);
            ctx.lineTo(x, y);
          }
          const nextCyc = i + 1 < ch.samples.length ? ch.samples[i + 1].cycle : this.maxCycle;
          ctx.lineTo(Math.min(w, toX(nextCyc)), y);
        }
        ctx.stroke();
      } else {
        // Multi-bit bus box representation
        for (let i = 0; i < ch.samples.length; i++) {
          const s = ch.samples[i];
          const x1 = Math.max(labelW, toX(s.cycle));
          const nextCyc = i + 1 < ch.samples.length ? ch.samples[i + 1].cycle : this.maxCycle;
          const x2 = Math.min(w, toX(nextCyc));
          if (x2 <= labelW) continue;
          ctx.fillStyle = '#f5faf5';
          ctx.fillRect(x1, yHigh, Math.max(2, x2 - x1), yLow - yHigh);
          ctx.strokeRect(x1, yHigh, Math.max(2, x2 - x1), yLow - yHigh);
          if (x2 - x1 > 46) {
            ctx.fillStyle = color;
            ctx.font = '10px Consolas, monospace';
            ctx.fillText(`0x${s.value.toString(16).toUpperCase()}`, x1 + 4, yTop + rowH / 2 + 3);
          }
        }
      }
      ctx.lineWidth = 1;
    });

    if (this.hoverX !== null && this.hoverX >= labelW) {
      ctx.strokeStyle = '#cc0000';
      ctx.beginPath();
      ctx.moveTo(this.hoverX, headerH);
      ctx.lineTo(this.hoverX, h);
      ctx.stroke();

      const cyc = startCycle + ((this.hoverX - labelW) / plotW) * visibleCycles;
      const vals = this.channels.map((ch) => {
        let v = 0;
        for (const s of ch.samples) {
          if (s.cycle <= cyc) v = s.value;
          else break;
        }
        return `${ch.signal}=0x${v.toString(16).toUpperCase()}`;
      });
      this.onHoverInfo(`Cycle ${Math.round(cyc)}  |  ${vals.join('   ')}`);
    }
  }
}
