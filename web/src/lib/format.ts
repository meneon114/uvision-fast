export function normPath(p: string): string {
  return p.replace(/\\/g, '/').toLowerCase();
}

export function samePath(a: string | null | undefined, b: string | null | undefined): boolean {
  return !!a && !!b && normPath(a) === normPath(b);
}

export function baseName(p: string): string {
  return p.replace(/\\/g, '/').split('/').pop() || p;
}

export function dirName(p: string): string {
  const parts = p.replace(/\\/g, '/').split('/');
  parts.pop();
  return parts.join('/');
}

// Route splats carry host paths without their leading slash:
// "/Blinky/blinky.uvprojx" <-> "Blinky/blinky.uvprojx", "C:/Labs/a.uvprojx" stays as is.
export function pathToSplat(p: string): string {
  return p.replace(/\\/g, '/').replace(/^\/+/, '');
}

export function splatToPath(s: string): string {
  const clean = decodeURIComponent(s).replace(/\\/g, '/');
  return /^[A-Za-z]:\//.test(clean) ? clean : '/' + clean.replace(/^\/+/, '');
}

export function hex32(n: number): string {
  return '0x' + (n >>> 0).toString(16).toUpperCase().padStart(8, '0');
}

export function hex16(n: number): string {
  return '0x' + (n & 0xffff).toString(16).toUpperCase().padStart(4, '0');
}

export function hex8(n: number): string {
  return (n & 0xff).toString(16).toUpperCase().padStart(2, '0');
}

export function parseNum(text: string): number | null {
  const t = text.trim();
  if (!t) return null;
  const v = /^0x/i.test(t) ? parseInt(t.slice(2), 16) : /^[0-9a-f]+h$/i.test(t) ? parseInt(t.slice(0, -1), 16) : parseInt(t, 10);
  return Number.isFinite(v) ? v >>> 0 : null;
}

export function isSourceFile(p: string): boolean {
  return /\.(s|asm|c|h)$/i.test(p);
}

export function isCFile(p: string): boolean {
  return /\.(c|h)$/i.test(p);
}

export function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
