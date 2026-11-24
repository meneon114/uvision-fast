import { createFileRoute } from '@tanstack/react-router';
import { useEffect, useRef } from 'react';
import { RouteError, RoutePending } from '../components/shell/RouteStates';
import { Workbench } from '../components/shell/Workbench';
import { host } from '../host/client';
import { splatToPath } from '../lib/format';
import { setSearch } from '../lib/nav';
import { projectSearchSchema } from '../lib/search';
import { adoptProject, ensureDoc, firstSource, startDebug, stopDebug } from '../state/ide';
import { useIde } from '../state/store';

// /project/<path to .uvprojx or a source file>?file=&line=&view=&panel=&mode=&mem=
export const Route = createFileRoute('/project/$')({
  validateSearch: projectSearchSchema,
  loaderDeps: () => ({}),
  loader: async ({ params }) => {
    const path = splatToPath(params._splat ?? '');
    if (!path) throw new Error('No project path in the address.');
    const project = await host().openProject(path);
    return { path, project };
  },
  staleTime: Infinity,
  shouldReload: false,
  pendingComponent: () => <Workbench editor={<RoutePending />} />,
  errorComponent: (props) => <Workbench editor={<RouteError {...props} />} />,
  component: ProjectRoute,
});

function ProjectRoute() {
  const { path, project } = Route.useLoaderData();
  const { file, mode } = Route.useSearch();
  const session = useIde((s) => s.session);
  const adopted = useIde((s) => s.projectPath === path && s.project !== null);
  const starting = useRef(false);

  useEffect(() => {
    adoptProject(path, project);
  }, [path, project]);

  // The URL is the source of truth for the open file.
  useEffect(() => {
    if (!adopted) return;
    if (file) void ensureDoc(file);
    else {
      const first = firstSource(useIde.getState().project);
      if (first) setSearch({ file: first }, true);
    }
  }, [adopted, file]);

  // ...and for the debug session.
  useEffect(() => {
    if (!adopted || starting.current) return;
    if (mode === 'debug' && session === 'off') {
      starting.current = true;
      void startDebug()
        .then((ok) => {
          if (!ok) setSearch({ mode: 'edit', view: 'project', panel: 'build' }, true);
        })
        .finally(() => {
          starting.current = false;
        });
    } else if (mode !== 'debug' && session === 'on') {
      void stopDebug();
    }
  }, [adopted, mode, session]);

  return <Workbench />;
}
