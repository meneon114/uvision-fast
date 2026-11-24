import { Tree, TreeGroup, TreeItem, TreeItemIndicator, TreeItemLabel } from '@ui-sdk/react/tree';
import { useMemo } from 'react';
import { baseName, normPath, samePath } from '../../lib/format';
import { setSearch } from '../../lib/nav';
import { useProjectSearch } from '../../lib/useProjectSearch';
import { useIde } from '../../state/store';
import { Icon } from '../Icon';
import { CmdButton } from '../shell/TbButton';
import { Section } from './Section';
import { ViewTitle } from './ViewTitle';

function fileIcon(t: number, p: string) {
  return t === 1 ? 'file-code' : t === 2 ? 'file-binary' : /\.h$/i.test(p) ? 'file-code' : 'file';
}

function labelsOf(content: string): { name: string; line: number }[] {
  const out: { name: string; line: number }[] = [];
  content.split('\n').forEach((l, i) => {
    // Possessive-style: `\s+(?=\S)` pins the lookahead to the next token, so backtracking
    // over the whitespace can't sneak an EQU/RN/DCx constant past it.
    const m = /^([A-Za-z_][\w]*)\b(\s+(?=\S)(?!EQU\b|RN\b|DCD\b|DCW\b|DCB\b|SPACE\b)|\s*$)/i.exec(l);
    if (m && !/^(AREA|END|EXPORT|IMPORT|THUMB|ALIGN|ENTRY|PRESERVE8)$/i.test(m[1])) out.push({ name: m[1], line: i + 1 });
  });
  return out;
}

export function ProjectView({ title }: { title: string }) {
  const project = useIde((s) => s.project);
  const path = useIde((s) => s.projectPath);
  const session = useIde((s) => s.session);
  const symbols = useIde((s) => s.symbols);
  const disasm = useIde((s) => s.disasm);
  const search = useProjectSearch();
  const activeDoc = useIde((s) => (search.file ? s.docs[normPath(search.file)] : undefined));

  const name = project?.project_file ? baseName(project.project_file).replace(/\.uvprojx?$/i, '') : path ? baseName(path) : 'No project';
  const target = project?.options.target_name || 'Target 1';
  const expanded = useMemo(() => ['proj', 'target', ...(project?.groups.map((g) => `g:${g.name}`) ?? [])], [project]);

  const functions = useMemo(() => {
    if (session === 'on' && symbols.length) {
      return symbols
        .filter((s) => s.is_func || s.name === '__Vectors')
        .map((s) => {
          const d = disasm.find((x) => x.addr === (s.addr & ~1));
          return { name: s.name, file: d?.file ?? null, line: d?.line ?? null };
        });
    }
    if (activeDoc) return labelsOf(activeDoc.content).map((l) => ({ name: l.name, file: activeDoc.path, line: l.line }));
    return [];
  }, [session, symbols, disasm, activeDoc]);

  return (
    <div className="view">
      <ViewTitle title={title}>
        <CmdButton id="project.addItem" />
        <CmdButton id="project.addExisting" />
        <CmdButton id="project.options" />
      </ViewTitle>
      <Section title={`Project: ${name}`} grow>
        {project ? (
          <Tree
            aria-label="Project files"
            className="tree"
            value={search.file ?? null}
            defaultExpanded={expanded}
            onAction={(v) => {
              if (v.startsWith('/') || /^[A-Za-z]:/.test(v)) setSearch({ file: v, line: undefined });
            }}
            activation="manual"
          >
            <TreeItem value="target">
              <TreeItemLabel>
                <TreeItemIndicator />
                <Icon name="target" className="fi" />
                <span className="rn">{target}</span>
              </TreeItemLabel>
              <TreeGroup>
                {project.groups.map((g) => (
                  <TreeItem key={g.name} value={`g:${g.name}`}>
                    <TreeItemLabel>
                      <TreeItemIndicator />
                      <Icon name="folder-opened" className="fi" />
                      <span className="rn">{g.name}</span>
                    </TreeItemLabel>
                    <TreeGroup>
                      {g.files.map((f) => (
                        <TreeItem key={f.abs_path} value={f.abs_path}>
                          <TreeItemLabel data-current={samePath(f.abs_path, search.file) || undefined}>
                            <Icon name={fileIcon(f.file_type, f.abs_path)} className="fi asm" />
                            <span className="rn">{f.name}</span>
                          </TreeItemLabel>
                        </TreeItem>
                      ))}
                    </TreeGroup>
                  </TreeItem>
                ))}
              </TreeGroup>
            </TreeItem>
          </Tree>
        ) : (
          <p className="empty">Open a project from File &gt; Open or a lab example.</p>
        )}
      </Section>
      <Section title="Functions">
        {functions.length ? (
          <Tree
            aria-label="Functions"
            className="tree"
            mode="none"
            onAction={(v) => {
              const f = functions[Number(v)];
              if (f?.file) setSearch({ file: f.file, line: f.line ?? undefined });
            }}
          >
            {functions.map((f, i) => (
              <TreeItem key={`${f.name}-${i}`} value={String(i)}>
                <TreeItemLabel>
                  <Icon name={/vector/i.test(f.name) ? 'symbol-variable' : 'symbol-method'} className="fi" />
                  <span className="rn">{f.name}</span>
                  {f.line ? <span className="rv plain">:{f.line}</span> : null}
                </TreeItemLabel>
              </TreeItem>
            ))}
          </Tree>
        ) : (
          <p className="empty">No labels in the open file.</p>
        )}
      </Section>
    </div>
  );
}
