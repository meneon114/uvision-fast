import { Collapsible, CollapsiblePanel, CollapsibleTrigger } from '@ui-sdk/react/collapsible';
import type { ReactNode } from 'react';
import { Icon } from '../Icon';

export function Section({
  title,
  children,
  defaultOpen = true,
  actions,
  grow,
}: {
  title: string;
  children: ReactNode;
  defaultOpen?: boolean;
  actions?: ReactNode;
  grow?: boolean;
}) {
  return (
    <Collapsible defaultOpen={defaultOpen} className="sect" data-grow={grow || undefined}>
      <div className="sh">
        <CollapsibleTrigger className="sh-trigger">
          <Icon name="chevron-right" className="sh-chev" />
          {title}
        </CollapsibleTrigger>
        {actions ? <span className="sh-actions">{actions}</span> : null}
      </div>
      <CollapsiblePanel className="sect-body">{children}</CollapsiblePanel>
    </Collapsible>
  );
}
