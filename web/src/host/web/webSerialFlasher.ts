// STM32 USART System Bootloader (AN3155) Flasher over WebSerial (115200 baud, 8E1)

const ACK = 0x79;
const NACK = 0x1f;

export interface Stm32FlashProgress {
  step: string;
  percent: number;
  log: string;
}

class BufferedSerialReader {
  private buf: number[] = [];
  private reader: ReadableStreamDefaultReader<Uint8Array>;

  constructor(port: any) {
    this.reader = port.readable.getReader();
  }

  async readBytes(count: number, timeoutMs = 2000): Promise<Uint8Array> {
    const deadline = Date.now() + timeoutMs;
    while (this.buf.length < count) {
      const rem = deadline - Date.now();
      if (rem <= 0) {
        throw new Error(`Serial timeout waiting for ${count} byte(s)`);
      }
      const timer = new Promise<{ timeout: true }>((res) => setTimeout(() => res({ timeout: true }), rem));
      const res: any = await Promise.race([this.reader.read(), timer]);
      if (res && res.timeout) {
        throw new Error('STM32 bootloader timed out. Ensure BOOT0=1, press RESET, and verify TX/RX wiring.');
      }
      if (res.done) {
        throw new Error('Serial port stream closed.');
      }
      if (res.value) {
        for (const b of res.value) this.buf.push(b);
      }
    }
    return new Uint8Array(this.buf.splice(0, count));
  }

  async waitAck(ctx: string, timeoutMs = 2500): Promise<void> {
    const [b] = await this.readBytes(1, timeoutMs);
    if (b === NACK) {
      throw new Error(`STM32 bootloader returned NACK at ${ctx}`);
    }
    if (b !== ACK) {
      throw new Error(`Unexpected bootloader byte 0x${b.toString(16)} at ${ctx} (expected 0x79 ACK)`);
    }
  }

  release(): void {
    try {
      this.reader.releaseLock();
    } catch {
      // ignore
    }
  }
}

export function isWebSerialSupported(): boolean {
  return typeof navigator !== 'undefined' && 'serial' in navigator;
}

export async function flashStm32ViaWebSerial(firmwareBin: Uint8Array, baseAddr: number, onProgress: (p: Stm32FlashProgress) => void): Promise<void> {
  if (!isWebSerialSupported()) {
    throw new Error('WebSerial API is not supported in this browser. Use Chromium / Chrome / Edge over HTTPS or localhost.');
  }
  const navSerial = (navigator as any).serial;
  onProgress({ step: 'Selecting serial port...', percent: 2, log: 'Requesting USB-UART adapter via WebSerial...' });
  const port = await navSerial.requestPort();
  await port.open({ baudRate: 115200, dataBits: 8, parity: 'even', stopBits: 1 });

  const writer = port.writable.getWriter();
  const reader = new BufferedSerialReader(port);

  const send = async (bytes: number[] | Uint8Array) => {
    const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    await writer.write(arr);
  };

  try {
    // 1. Sync 0x7F
    onProgress({ step: 'Syncing bootloader...', percent: 8, log: 'Sending 0x7F auto-baud sync byte (BOOT0=1)...' });
    await send([0x7f]);
    await reader.waitAck('sync (0x7F)', 3000);

    // 2. Get commands (0x00) to check Extended Erase (0x44) vs Erase (0x43)
    await send([0x00, 0xff]);
    await reader.waitAck('GET command');
    const [nMinus1] = await reader.readBytes(1);
    const cmds = await reader.readBytes(nMinus1 + 1);
    await reader.waitAck('GET tail');
    const hasExtErase = Array.from(cmds).includes(0x44);

    // 3. Get Chip ID (0x02)
    let chipIdHex = 'unknown';
    try {
      await send([0x02, 0xfd]);
      await reader.waitAck('GET ID');
      const [idN] = await reader.readBytes(1);
      const idBytes = await reader.readBytes(idN + 1);
      await reader.waitAck('GET ID tail');
      chipIdHex =
        '0x' +
        Array.from(idBytes)
          .map((b) => b.toString(16).padStart(2, '0'))
          .join('');
    } catch {
      // optional
    }

    onProgress({
      step: 'Erasing flash...',
      percent: 18,
      log: `Connected! STM32 PID=${chipIdHex}. Erasing flash (${hasExtErase ? 'Extended' : 'Standard'})...`,
    });

    // 4. Mass Erase
    if (hasExtErase) {
      await send([0x44, 0xbb]);
      await reader.waitAck('EXT_ERASE cmd');
      await send([0xff, 0xff, 0x00]); // Global mass erase + checksum (0xFF ^ 0xFF = 0x00)
      await reader.waitAck('EXT_ERASE mass', 15000);
    } else {
      await send([0x43, 0xbc]);
      await reader.waitAck('ERASE cmd');
      await send([0xff, 0x00]); // Global erase
      await reader.waitAck('ERASE mass', 15000);
    }

    // 5. Write Memory in <=256 byte blocks (word-aligned length)
    const total = firmwareBin.length;
    let offset = 0;
    while (offset < total) {
      const rawChunk = firmwareBin.subarray(offset, Math.min(offset + 256, total));
      const alignedLen = (rawChunk.length + 3) & ~3;
      const chunk = new Uint8Array(alignedLen);
      chunk.fill(0xff);
      chunk.set(rawChunk);

      const addr = (baseAddr + offset) >>> 0;
      const addrBytes = [(addr >>> 24) & 0xff, (addr >>> 16) & 0xff, (addr >>> 8) & 0xff, addr & 0xff];
      const addrCsum = addrBytes[0] ^ addrBytes[1] ^ addrBytes[2] ^ addrBytes[3];

      await send([0x31, 0xce]);
      await reader.waitAck('WRITE cmd');
      await send([...addrBytes, addrCsum]);
      await reader.waitAck(`WRITE addr 0x${addr.toString(16)}`);

      const nByte = alignedLen - 1;
      let dataCsum = nByte;
      for (const b of chunk) dataCsum ^= b;
      const payload = new Uint8Array(alignedLen + 2);
      payload[0] = nByte;
      payload.set(chunk, 1);
      payload[alignedLen + 1] = dataCsum & 0xff;

      await send(payload);
      await reader.waitAck(`WRITE data @ 0x${addr.toString(16)}`, 5000);

      offset += rawChunk.length;
      const pct = 20 + Math.round((offset / total) * 75);
      onProgress({
        step: `Programming ${offset}/${total} bytes...`,
        percent: pct,
        log: `Wrote ${chunk.length} bytes @ 0x${addr.toString(16).toUpperCase().padStart(8, '0')}`,
      });
    }

    // 6. Jump to Reset vector (Go command 0x21)
    const goAddr = baseAddr >>> 0;
    const goBytes = [(goAddr >>> 24) & 0xff, (goAddr >>> 16) & 0xff, (goAddr >>> 8) & 0xff, goAddr & 0xff];
    const goCsum = goBytes[0] ^ goBytes[1] ^ goBytes[2] ^ goBytes[3];
    await send([0x21, 0xde]);
    await reader.waitAck('GO cmd');
    await send([...goBytes, goCsum]);
    await reader.waitAck('GO addr');

    onProgress({
      step: 'Completed!',
      percent: 100,
      log: `Flashed ${total} bytes to 0x${baseAddr.toString(16).toUpperCase()} and jumped to application.`,
    });
  } finally {
    try {
      writer.releaseLock();
    } catch {}
    reader.release();
    try {
      await port.close();
    } catch {}
  }
}
