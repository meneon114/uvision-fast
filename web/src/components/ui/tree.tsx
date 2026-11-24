import * as T from '@ui-sdk/react/tree';
import type { ComponentProps } from 'react';
import { cx } from '../../lib/cx';
import { Icon } from '../Icon';
import { styled } from './styled';

// Dense, VS Code style rows for the project, registers, watch and find trees.
// Groups are styled from here: the tree finds a branch's TreeGroup by its component type,
// so TreeGroup itself must stay ui-sdk's own.
export const Tree = styled(T.Tree, 'max-w-none gap-0 py-0.5 text-ui **:data-[slot=tree-group]:gap-0 **:data-[slot=tree-group]:m-0', 'Tree');
export const TreeItemIndicator = styled(T.TreeItemIndicator, 'w-4 text-muted', 'TreeItemIndicator');
export { TreeGroup, TreeItem } from '@ui-sdk/react/tree';

const LABEL =
  'group/label h-5.5 gap-1 rounded-none ps-[calc(10px+var(--ui-tree-indent)*0.9)] pe-2.5 text-fg2 not-data-selected:hover:bg-hover data-selected:bg-sel data-selected:shadow-[inset_2px_0_0_var(--selbar)] [:focus-visible>&]:outline-1 [:focus-visible>&]:outline-acc [:focus-visible>&]:-outline-offset-1';

// `flat` drops the room a leaf keeps for a branch's chevron.
export function TreeItemLabel({
  flat,
  className,
  ...props
}: Omit<ComponentProps<typeof T.TreeItemLabel>, 'className'> & { flat?: boolean; className?: string }) {
  return (
    <T.TreeItemLabel
      {...props}
      className={cx(LABEL, flat ? 'not-data-branch:ps-1.5' : 'not-data-branch:ps-[calc(28px+var(--ui-tree-indent)*0.9)]', className)}
    />
  );
}

export function TreeIcon({ name, tone = 'muted' }: { name: string; tone?: 'muted' | 'acc' | 'asm' }) {
  return <Icon name={name} className={cx('text-md', tone === 'acc' ? 'text-acc' : tone === 'asm' ? 'text-asm' : 'text-muted')} />;
}

// A row's name and value.
export function RowName({ group, mono, title, children }: { group?: boolean; mono?: boolean; title?: string; children: React.ReactNode }) {
  return (
    <span
      title={title}
      className={cx('flex-1 min-w-0 overflow-hidden text-ellipsis', group ? 'font-semibold text-fg' : 'text-fg2', mono && 'font-mono text-ui')}
    >
      {children}
    </span>
  );
}

export function RowValue({
  plain,
  changed,
  error,
  title,
  children,
}: {
  plain?: boolean;
  changed?: boolean;
  error?: boolean;
  title?: string;
  children: React.ReactNode;
}) {
  return (
    <span
      title={title}
      className={cx(
        'text-sm whitespace-nowrap',
        plain || error ? 'font-ui' : 'font-mono',
        plain && 'overflow-hidden text-ellipsis',
        error ? 'text-bp' : changed ? 'text-chg font-semibold' : plain ? 'text-muted' : 'text-fg',
      )}
    >
      {children}
    </span>
  );
}
