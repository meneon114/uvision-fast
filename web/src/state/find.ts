import { create } from 'zustand';
import { host } from '../host/client';
import { errMsg, normPath } from '../lib/format';
import { projectFiles } from './ide';
import { ide } from './store';

export interface FindHit {
  file: string;
  line: number;
  col: number;
  text: string;
}

interface FindState {
  query: string;
  matchCase: boolean;
  regex: boolean;
  busy: boolean;
  hits: FindHit[];
  error: string | null;
  searched: string | null;
}

export const useFind = create<FindState>()(() => ({
  query: '',
  matchCase: false,
  regex: false,
  busy: false,
  hits: [],
  error: null,
  searched: null,
}));

// Searches every source file of the open project, using unsaved editor text where there is some.
export async function findInFiles(): Promise<void> {
  const { query, matchCase, regex } = useFind.getState();
  if (!query) {
    useFind.setState({ hits: [], searched: null, error: null });
    return;
  }
  let re: RegExp;
  try {
    re = new RegExp(regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), matchCase ? 'g' : 'gi');
  } catch (e) {
    useFind.setState({ error: errMsg(e) });
    return;
  }
  useFind.setState({ busy: true, error: null });
  const st = ide.get();
  const files = projectFiles(st.project);
  if (!files.length && st.tabs.length) files.push(...st.tabs);
  const hits: FindHit[] = [];
  for (const f of files) {
    let text = st.docs[normPath(f)]?.content;
    if (text === undefined) {
      try {
        text = (await host().readFile(f)).content;
      } catch {
        continue;
      }
    }
    text.split('\n').forEach((l, i) => {
      re.lastIndex = 0;
      const m = re.exec(l);
      if (m) hits.push({ file: f, line: i + 1, col: m.index + 1, text: l.trim().slice(0, 200) });
    });
    if (hits.length > 2000) break;
  }
  useFind.setState({ hits, busy: false, searched: query });
}
