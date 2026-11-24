import { Toolbar } from '@ui-sdk/react/toolbar';
import type { ReactNode } from 'react';

export function ViewTitle({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="vtitle">
      <h2>{title}</h2>
      <span className="grow" />
      {children ? (
        <Toolbar aria-label={`${title} actions`} className="vtitle-actions">
          {children}
        </Toolbar>
      ) : null}
    </div>
  );
}
