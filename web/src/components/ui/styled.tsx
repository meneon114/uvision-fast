import type { ComponentType } from 'react';
import { cx } from '../../lib/cx';

// A ui-sdk part with the app's classes on it. Extra classes are appended, so pass only
// ones that don't fight the base set.
export function styled<P extends { className?: unknown }>(Base: ComponentType<P>, base: string, name: string) {
  function Styled({ className, ...props }: Omit<P, 'className'> & { className?: string }) {
    return <Base {...(props as unknown as P)} className={cx(base, className)} />;
  }
  Styled.displayName = name;
  return Styled;
}
