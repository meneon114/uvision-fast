import type { ErrorComponentProps } from '@tanstack/react-router';
import { Button } from '../ui/Button';
import { errMsg } from '../../lib/format';
import { Icon } from '../Icon';

const STATE = 'flex-1 flex flex-col items-center justify-center gap-2.5 p-6 bg-bg text-center';

export function RouteError({ error, reset }: ErrorComponentProps) {
  return (
    <div className={STATE} role="alert">
      <Icon name="error" className="text-5xl text-bp" />
      <h2 className="text-xl font-semibold text-fg">Could not open this target</h2>
      <p className="max-w-140 font-mono text-ui wrap-anywhere">{errMsg(error)}</p>
      <div className="flex items-center gap-2 flex-wrap">
        <Button onClick={() => reset()}>Try again</Button>
        <Button onClick={() => (location.hash = '#/')}>Back to start</Button>
      </div>
    </div>
  );
}

export function RoutePending() {
  return (
    <div className={STATE} aria-busy="true">
      <Icon name="loading" className="text-5xl text-muted animate-spin" />
      <p className="max-w-140 wrap-anywhere">Opening project...</p>
    </div>
  );
}
