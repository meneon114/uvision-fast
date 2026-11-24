import { Checkbox, CheckboxIndicator } from '@ui-sdk/react/checkbox';
import { Input } from '@ui-sdk/react/input';
import { findInFiles, useFind } from '../../state/find';
import { showPanel } from '../../state/ide';
import { FindResults } from '../panel/FindResults';
import { ViewTitle } from './ViewTitle';
import { TbButton } from '../shell/TbButton';

export function FindView({ title }: { title: string }) {
  const { query, matchCase, regex, busy, hits, error, searched } = useFind();
  return (
    <div className="view">
      <ViewTitle title={title}>
        <TbButton label="Show results in panel" icon="list-flat" onClick={() => showPanel('find')} />
        <TbButton label="Clear results" icon="clear-all" onClick={() => useFind.setState({ query: '', hits: [], searched: null })} />
      </ViewTitle>
      <form
        className="find-form"
        onSubmit={(e) => {
          e.preventDefault();
          void findInFiles();
        }}
      >
        <Input aria-label="Find in project files" placeholder="Find (Enter to search)" value={query} onValueChange={(v) => useFind.setState({ query: v })} />
        <div className="row">
          <label className="check">
            <Checkbox
              checked={matchCase}
              onCheckedChange={(c) => {
                const v = c === true;
                useFind.setState({ matchCase: v });
              }}
            >
              <CheckboxIndicator />
            </Checkbox>
            Match case
          </label>
          <label className="check">
            <Checkbox
              checked={regex}
              onCheckedChange={(c) => {
                const v = c === true;
                useFind.setState({ regex: v });
              }}
            >
              <CheckboxIndicator />
            </Checkbox>
            Regular expression
          </label>
        </div>
        {error ? <p className="err small">{error}</p> : null}
        <p className="muted small" role="status">
          {busy
            ? 'Searching...'
            : searched !== null
              ? `${hits.length} result${hits.length === 1 ? '' : 's'} for "${searched}"`
              : 'Searches all files in the project.'}
        </p>
      </form>
      <div className="view-scroll">
        <FindResults />
      </div>
    </div>
  );
}
