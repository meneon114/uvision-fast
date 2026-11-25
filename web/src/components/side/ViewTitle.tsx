import { Toolbar } from '@ui-sdk/react/toolbar';
import type { ReactNode } from 'react';
import { TbSize } from '../shell/TbButton';

export function ViewTitle({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="h-8.75 flex items-center gap-1.5 pl-5 pr-2 flex-none">
      <h2 className="text-2xs font-normal tracking-[0.04em] uppercase text-muted">{title}</h2>
      <span className="flex-1" />
      {children ? (
        <TbSize value="sm">
          <Toolbar aria-label={`${title} actions`} className="flex items-center gap-px p-0 border-0 bg-transparent">
            {children}
          </Toolbar>
        </TbSize>
      ) : null}
    </div>
  );
}
