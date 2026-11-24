// The editor's tab strip, shared by the source group and the disassembly.

export const GROUP = 'flex-1 flex flex-col min-w-0 min-h-0 bg-bg';

export const TAB_BAR = 'h-8.75 flex flex-none bg-chrome border-b border-border overflow-x-auto overflow-y-hidden no-scrollbar';

// The active tab joins the editor: page background, accent top edge, and it covers the strip's border.
export const TAB_WRAP =
  'group/tab relative flex items-center flex-none border-r border-border bg-chrome data-active:bg-bg data-active:shadow-[inset_0_1px_0_var(--acc)] data-active:-mb-px';

export const TAB =
  'h-full flex items-center gap-1.5 m-0 pl-3 pr-1 border-0 bg-transparent text-muted text-ui font-normal cursor-pointer whitespace-nowrap group-data-active/tab:text-fg';
