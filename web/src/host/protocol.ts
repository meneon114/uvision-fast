// Shared by the UI and the VS Code extension host. The extension forwards only
// these methods to uvcli, so the webview can't reach anything else the CLI serves.
export const UI_RPC_METHODS = [
  'toolchain.info',
  'project.open',
  'project.new',
  'project.addFile',
  'project.saveOptions',
  'build.run',
  'build.artifacts',
  'debug.start',
  'debug.stop',
  'debug.reset',
  'debug.step',
  'debug.stepOver',
  'debug.stepOut',
  'debug.run',
  'debug.pause',
  'debug.state',
  'debug.runTo',
  'debug.setBreakpoint',
  'debug.clearBreakpoints',
  'debug.disassembly',
  'debug.memoryRead',
  'debug.memoryWrite',
  'debug.setRegister',
  'debug.evalWatch',
  'debug.command',
  'debug.peripherals',
  'debug.setGpioPin',
  'debug.uartSend',
  'debug.logicAdd',
  'debug.logicRemove',
  'debug.logicSamples',
  'pack.list',
  'flash.program',
] as const;

export type RpcMethod = (typeof UI_RPC_METHODS)[number];

export const VSCODE_HOST_ACTIONS = [
  'readFile',
  'writeFile',
  'pickOpenProjectOrFile',
  'pickNewProjectDir',
  'pickExistingSourceFile',
  'promptText',
  // Picks a .pack with the native dialog and runs pack.add on the host side,
  // so the webview never names a pack path itself.
  'installPack',
] as const;

export type UiToHost =
  | { type: 'ready' }
  | { type: 'rpc'; reqId: number; method: string; params: Record<string, unknown> }
  | { type: 'host'; reqId: number; action: string; payload: Record<string, unknown> };

export type HostToUi =
  { type: 'rpcResponse'; reqId: number; result?: unknown; error?: string } | { type: 'hostResponse'; reqId: number; result?: unknown; error?: string };
