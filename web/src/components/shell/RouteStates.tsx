import type { ErrorComponentProps } from '@tanstack/react-router';
import { Button } from '@ui-sdk/react/button';
import { errMsg } from '../../lib/format';
import { Icon } from '../Icon';

export function RouteError({ error, reset }: ErrorComponentProps) {
  return (
    <div className="route-state" role="alert">
      <Icon name="error" className="route-state-icon err" />
      <h2>Could not open this target</h2>
      <p className="mono">{errMsg(error)}</p>
      <div className="row">
        <Button onClick={() => reset()}>Try again</Button>
        <Button onClick={() => (location.hash = '#/')}>Back to start</Button>
      </div>
    </div>
  );
}

export function RoutePending() {
  return (
    <div className="route-state" aria-busy="true">
      <Icon name="loading" className="route-state-icon spin" />
      <p>Opening project...</p>
    </div>
  );
}
