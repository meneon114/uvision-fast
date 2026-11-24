// Gutter marks shared by the CodeMirror breakpoint gutter (cm.ts) and the disassembly listing.

// One 22 x 19px gutter cell.
export const GUT = 'relative w-5.5 h-[19px] flex items-center justify-center cursor-pointer';

// On a clickable cell without a breakpoint, hovering previews one.
export const GUT_HINT =
  "not-has-data-bp:hover:after:content-[''] not-has-data-bp:hover:after:size-3 not-has-data-bp:hover:after:rounded-full not-has-data-bp:hover:after:bg-bp/40";

// The bar on lines that have executed.
export const EXEC_MARK = 'absolute left-0.5 inset-y-0.5 w-0.75 rounded-[1px] bg-cov opacity-75';

// A breakpoint dot. Carries data-bp so the cell can tell it has one.
export const BP_MARK = 'size-3 rounded-full bg-bp shadow-[0_0_0_1px_color-mix(in_srgb,var(--bp)_60%,black)]';

// The arrow on the program counter's line.
export const PC_MARK = 'absolute text-pcline text-xl drop-shadow-[0_0_1px_rgb(0_0_0/0.45)]';
