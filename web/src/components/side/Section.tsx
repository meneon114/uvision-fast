import { Collapsible, CollapsiblePanel, CollapsibleTrigger } from '@ui-sdk/react/collapsible';
import type { ReactNode } from 'react';
import { Icon } from '../Icon';

// A collapsible side bar section. The chevron turns when the trigger is expanded.
export function Section({ title, children, defaultOpen = true, actions }: { title: string; children: ReactNode; defaultOpen?: boolean; actions?: ReactNode }) {
  return (
    <Collapsible defaultOpen={defaultOpen} className="border-t border-border first:border-t-0">
      <div className="h-6 flex items-center pr-1.5">
        <CollapsibleTrigger className="group/sh flex-1 h-full flex items-center gap-0.5 px-1.5 border-0 bg-transparent text-fg text-2xs font-bold tracking-[0.03em] uppercase cursor-pointer text-left after:hidden!">
          <Icon
            name="chevron-right"
            className="text-xl transition-transform duration-120 ease-snap group-aria-expanded/sh:rotate-90 group-data-panel-open/sh:rotate-90"
          />
          {title}
        </CollapsibleTrigger>
        {actions ? <span>{actions}</span> : null}
      </div>
      <CollapsiblePanel className="pb-1">{children}</CollapsiblePanel>
    </Collapsible>
  );
}
