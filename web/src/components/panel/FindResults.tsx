import { Tree, TreeGroup, TreeItem, TreeItemIndicator, TreeItemLabel } from '@ui-sdk/react/tree';
import { useMemo } from 'react';
import { baseName } from '../../lib/format';
import { setSearch } from '../../lib/nav';
import { useFind, type FindHit } from '../../state/find';
import { Icon } from '../Icon';

export function FindResults() {
  const hits = useFind((s) => s.hits);
  const byFile = useMemo(() => {
    const m = new Map<string, { hit: FindHit; i: number }[]>();
    hits.forEach((h, i) => m.set(h.file, [...(m.get(h.file) ?? []), { hit: h, i }]));
    return [...m.entries()];
  }, [hits]);
  if (!hits.length) return null;
  return (
    <Tree
      aria-label="Find results"
      className="tree find-tree"
      mode="none"
      defaultExpanded={byFile.map(([f]) => f)}
      onAction={(v) => {
        const h = hits[Number(v)];
        if (h) setSearch({ file: h.file, line: h.line });
      }}
    >
      {byFile.map(([file, list]) => (
        <TreeItem key={file} value={file}>
          <TreeItemLabel>
            <TreeItemIndicator />
            <Icon name="file-code" className="fi asm" />
            <span className="rn">{baseName(file)}</span>
            <span className="badge">{list.length}</span>
          </TreeItemLabel>
          <TreeGroup>
            {list.map(({ hit, i }) => (
              <TreeItem key={i} value={String(i)}>
                <TreeItemLabel>
                  <span className="rv plain">{hit.line}</span>
                  <span className="rn mono">{hit.text}</span>
                </TreeItemLabel>
              </TreeItem>
            ))}
          </TreeGroup>
        </TreeItem>
      ))}
    </Tree>
  );
}
