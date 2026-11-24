# uVision Fast - Keil µVision 5 IDE (WebAssembly PWA, VS Code Extension & Rust CLI)

A high-speed, zero-lag recreation of **Keil µVision 5** for **ARM Cortex-M3 / STM32F1** assembly & firmware development, available as:

1. **Standalone Offline WebAssembly PWA** - Runs directly in your browser and installs offline:
   - **Live URL**: [https://meneon114.github.io/uvision-fast/](https://meneon114.github.io/uvision-fast/)
2. **Self-Contained VS Code Extension (`.vsix`)** - the same Keil µVision 5 style UI inside VS Code, bundling the native `uvcli.exe` engine and portable OpenOCD (download from [GitHub Releases](https://github.com/meneon114/uvision-fast/releases)).
3. **Native Rust CLI (`uvcli`)** - Build, simulate, interactive debug (`uvcli debug`), and flash real STM32 boards (`uvcli flash`).

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

### 2. Web UI and offline PWA (`web/`)

The PWA and the VS Code webview are one React app (React 19, Vite, TanStack Router, ui-sdk, CodeMirror 6) in `web/src`. Only the host differs: the PWA runs the WebAssembly core in the page, and the extension forwards the same JSON-RPC calls to `uvcli`.

```bash
cd web
npm ci
npm run build:wasm     # builds uvision_core.wasm with cargo and copies it into public/
npm run gen:icons      # optional: regenerates the PWA icons
npm run build          # type-checks the app and the service worker, then builds web/dist
npm run preview        # serves web/dist on http://localhost:4173
```

`npm run dev` starts the Vite dev server. The service worker is only built by `npm run build`, so test offline behaviour against `npm run preview`: load the page once, then go offline and reload. The worker precaches every asset, fails its install if any entry fails to download, shows a "Reload" toast when an update is waiting, and deletes only its own caches.

### 3. VS Code Extension (`extension/`)

The extension build also builds the webview, so install `web/` dependencies first.

```bash
cd web && npm ci && cd ..
cd extension
npm ci
npm run build          # dev build: extension host + webview (vite build --mode vscode)
npx @vscode/vsce package   # runs vscode:prepublish, the production build
```

`npm run build` copies `cli/target/release/uvcli.exe` and a sibling `openocd/` into `bin/` when they exist, and only warns when they don't. The production build (`vscode:prepublish`) fails if `uvcli.exe` is missing, so a `.vsix` never ships with an empty `bin/`.

The webview can call only the JSON-RPC methods listed in `web/src/host/protocol.ts` (`UI_RPC_METHODS`), and every file path it sends must be inside a workspace folder, the open project, or a file or folder the user picked in a VS Code dialog.
