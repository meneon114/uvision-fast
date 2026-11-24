import { RowName, RowValue, Tree, TreeGroup, TreeIcon, TreeItem, TreeItemIndicator, TreeItemLabel } from '../ui/tree';
import { useMemo } from 'react';
import { baseName } from '../../lib/format';
import { setSearch } from '../../lib/nav';
import { useFind, type FindHit } from '../../state/find';

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
            <TreeIcon name="file-code" tone="asm" />
            <RowName>{baseName(file)}</RowName>
            <span>{list.length}</span>
          </TreeItemLabel>
          <TreeGroup>
            {list.map(({ hit, i }) => (
              <TreeItem key={i} value={String(i)}>
                <TreeItemLabel>
                  <RowValue plain>{hit.line}</RowValue>
                  <RowName mono>{hit.text}</RowName>
                </TreeItemLabel>
              </TreeItem>
            ))}
          </TreeGroup>
        </TreeItem>
      ))}
    </Tree>
  );
}
