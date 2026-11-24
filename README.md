# uVision Fast — Keil µVision 5 IDE (WebAssembly PWA, VS Code Extension & Rust CLI)

A high-speed, zero-lag recreation of **Keil µVision 5** for **ARM Cortex-M3 / STM32F1** assembly & firmware development — available as:

1. **Standalone Offline WebAssembly PWA** — Runs directly in your browser and installs offline:
   - **Live URL**: [https://meneon114.github.io/uvision-fast/](https://meneon114.github.io/uvision-fast/)
2. **Self-Contained VS Code Extension (`.vsix`)** — Classic Keil µVision 5 Light UI inside VS Code, bundling the native `uvcli.exe` engine and portable OpenOCD (download from [GitHub Releases](https://github.com/meneon114/uvision-fast/releases)).
3. **Native Rust CLI (`uvcli`)** — Build, simulate, interactive debug (`uvcli debug`), and flash real STM32 boards (`uvcli flash`).

---

## Features

- **Classic Keil µVision 5 Interface**: Exact Edit Mode ↔ Debug Mode layout, menu bar, toolbars, CodeMirror 6 editor with line-level breakpoints and yellow PC marker, Registers tree with blue change highlights and expandable `xPSR` flags, Disassembly, Memory 1–4 editable hex/ASCII grids, Watch 1, Call Stack, Command REPL, UART #1, Logic Analyzer, System Viewer, and Target Options.
- **Built-in Pure-Rust ARM Cortex-M3 Engine (`uvision_core.wasm` / `uvcli`)**:
  - Built-in Keil-syntax (`AREA`, `THUMB`, `DCD`, `EQU`, `LTORG`, `LDR =expr`) Thumb-16 / Thumb-2 assembler emitting ELF32 + DWARF v2 debug lines + Intel `.hex` + `.bin`.
  - Built-in Thumb-16 / Thumb-2 disassembler.
  - Cortex-M3 CPU + STM32F1 peripheral simulator (RCC, GPIOA–G with interactive LEDs & pin toggles, TIM1–4, USART1–3, SysTick/NVIC, and Logic Analyzer waveforms).
- **Offline Progressive Web App (PWA)**:
  - Pre-caches HTML, CSS, JS, and `uvision_core.wasm` via a generated Service Worker (`sw.js`) for 100% offline execution.
  - Persists projects and files locally in browser **IndexedDB**, with drag-and-drop import and `.s`/`.uvprojx`/`.hex`/`.bin`/`.axf` export.
- **Real STM32 Hardware Flashing**:
  - **Web App**: WebSerial STM32 UART System Bootloader (AN3155, `115200 8E1` via `PA9`/`PA10` + `BOOT0=1`) plus `.hex`/`.bin`/`.axf` download.
  - **Desktop / VS Code**: Bundled OpenOCD SWD flashing & hardware debugging via ST-Link (`uvcli flash` / `uvcli debug --hw`).

> **Note on C files**: ARM Assembly (`.s`/`.asm`) builds completely self-contained in both the WebAssembly PWA and desktop extension without Keil MDK. C compilation requires desktop `uvcli` + Keil MDK or Arm GNU Toolchain (`arm-none-eabi-gcc`).

---

## Building from Source

### 1. Rust CLI & WebAssembly Core (`cli/`)

```bash
cd cli
cargo test
cargo build --release --bin uvcli

# Compile WebAssembly engine
rustup target add wasm32-unknown-unknown
cargo build --lib --target wasm32-unknown-unknown --release
```

### 2. Standalone Offline PWA (`web/`)

```bash
cd web
npm install
npm run build:wasm
npm run gen:icons
npm run build
npm run preview
```

### 3. VS Code Extension (`extension/`)

```bash
cd extension
npm install
npm run build
```
