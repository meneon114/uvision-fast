import { openSearchPanel } from '@codemirror/search';
import { EditorState, Transaction } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { useEffect, useRef } from 'react';
import { normPath, samePath } from '../../lib/format';
import { editDoc, saveDoc, toggleBreakpoint } from '../../state/ide';
import { getDoc, ide, useIde } from '../../state/store';
import { baseExtensions, external, langCompartment, languageFor, setGutterState, type EditorCallbacks } from './cm';

// One EditorView, one EditorState per open file, so undo history survives tab switches.
const states = new Map<string, EditorState>();
let activeView: EditorView | null = null;

// Module-level so cached states, whose extensions close over these, stay valid
// across editor remounts. There is one source editor on screen at a time.
const currentPath = { current: '' };
const cb: { current: EditorCallbacks } = {
  current: {
    onChange: (c) => editDoc(currentPath.current, c),
    onToggleBreakpoint: (l) => void toggleBreakpoint(currentPath.current, l),
    onCursor: (l, c) => ide.set({ cursor: { line: l, col: c } }),
    onSave: () => void saveDoc(currentPath.current),
  },
};
let exts: ReturnType<typeof baseExtensions> | null = null;
const extensions = () => (exts ??= baseExtensions(cb));

export function openFindInEditor(): boolean {
  if (!activeView) return false;
  openSearchPanel(activeView);
  return true;
}

export function getActiveView(): EditorView | null {
  return activeView;
}

export function focusEditor(): void {
  activeView?.focus();
}

export function forgetEditorState(path: string): void {
  states.delete(normPath(path));
}

export function CodeEditor({ path, line }: { path: string; line?: number }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const current = currentPath;
  if (!current.current) current.current = path;

  const doc = useIde((s) => s.docs[normPath(path)]);
  const bps = useIde((s) => s.breakpoints[normPath(path)]);
  const session = useIde((s) => s.session);
  const execLines = useIde((s) => s.execLines);
  const dbg = useIde((s) => s.dbg);

  const makeState = (p: string, content: string) => EditorState.create({ doc: content, extensions: [...extensions(), langCompartment.of(languageFor(p))] });

  useEffect(() => {
    if (!host.current) return;
    current.current = path;
    const d = getDoc(path);
    const cached = states.get(normPath(path));
    const initial = cached && d && cached.doc.toString() === d.content ? cached : makeState(path, d?.content ?? '');
    const v = new EditorView({ state: initial, parent: host.current });
    view.current = v;
    activeView = v;
    return () => {
      states.set(normPath(current.current), v.state);
      v.destroy();
      if (activeView === v) activeView = null;
      view.current = null;
    };
  }, []);

  // Switch documents.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    if (!samePath(current.current, path)) {
      states.set(normPath(current.current), v.state);
      current.current = path;
      const d = getDoc(path);
      const cached = states.get(normPath(path));
      v.setState(cached && d && cached.doc.toString() === d.content ? cached : makeState(path, d?.content ?? ''));
    }
  }, [path]);

  // Content changed outside the editor (load, reload from disk, external write).
  useEffect(() => {
    const v = view.current;
    if (!v || !doc || !samePath(current.current, path)) return;
    const now = v.state.doc.toString();
    if (now !== doc.content) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: doc.content }, annotations: [external.of(true), Transaction.addToHistory.of(false)] });
    }
  }, [doc?.content, path]);

  // Gutter: breakpoints, executable lines and the PC arrow.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    const key = normPath(path);
    const exec =
      session === 'on' ? Object.entries(execLines).find(([f]) => normPath(f) === key || normPath(f).endsWith('/' + key.split('/').pop())) : undefined;
    const pcLine = session === 'on' && dbg && samePath(dbg.current_file, path) ? dbg.current_line : null;
    v.dispatch({ effects: setGutterState.of({ pcLine, breakpoints: new Set(bps ?? []), execLines: new Set(exec?.[1] ?? []) }) });
  }, [bps, session, execLines, dbg?.current_file, dbg?.current_line, path, doc !== undefined]);

  // Reveal the line named in the URL.
  useEffect(() => {
    const v = view.current;
    if (!v || !line || !doc) return;
    const n = Math.max(1, Math.min(line, v.state.doc.lines));
    const l = v.state.doc.line(n);
    if (v.state.doc.lineAt(v.state.selection.main.head).number !== n || session === 'on') {
      v.dispatch({ selection: { anchor: l.from }, effects: EditorView.scrollIntoView(l.from, { y: 'center' }) });
    }
  }, [line, path, doc !== undefined, dbg?.pc]);

  return <div ref={host} className="cm-host" data-file={path} />;
}
