import { Check } from '../ui/Check';
import { Input } from '../ui/Input';
import { Row, VIEW, VIEW_SCROLL } from '../ui/kit';
import { findInFiles, useFind } from '../../state/find';
import { showPanel } from '../../state/ide';
import { FindResults } from '../panel/FindResults';
import { ViewTitle } from './ViewTitle';
import { TbButton } from '../shell/TbButton';

export function FindView({ title }: { title: string }) {
  const { query, matchCase, regex, busy, hits, error, searched } = useFind();
  return (
    <div className={VIEW}>
      <ViewTitle title={title}>
        <TbButton label="Show results in panel" icon="list-flat" onClick={() => showPanel('find')} />
        <TbButton label="Clear results" icon="clear-all" onClick={() => useFind.setState({ query: '', hits: [], searched: null })} />
      </ViewTitle>
      <form
        className="flex flex-col gap-1.5 pt-0 pr-3 pb-2 pl-5"
        onSubmit={(e) => {
          e.preventDefault();
          void findInFiles();
        }}
      >
        <Input aria-label="Find in project files" placeholder="Find (Enter to search)" value={query} onValueChange={(v) => useFind.setState({ query: v })} />
        <Row>
          <Check checked={matchCase} onChange={(v) => useFind.setState({ matchCase: v })}>
            Match case
          </Check>
          <Check checked={regex} onChange={(v) => useFind.setState({ regex: v })}>
            Regular expression
          </Check>
        </Row>
        {error ? <p className="my-[1em] text-bp text-xs">{error}</p> : null}
        <p className="my-[1em] text-muted text-xs" role="status">
          {busy
            ? 'Searching...'
            : searched !== null
              ? `${hits.length} result${hits.length === 1 ? '' : 's'} for "${searched}"`
              : 'Searches all files in the project.'}
        </p>
      </form>
      <div className={VIEW_SCROLL}>
        <FindResults />
      </div>
    </div>
  );
}
