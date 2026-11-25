import type { AnyRouter } from '@tanstack/react-router';
import { isSourceFile, pathToSplat, splatToPath } from './format';
import type { ProjectSearch } from './search';

// Set once from main.tsx; kept out of module scope imports to avoid a cycle
// between the route tree and the actions the routes call.
let routerRef: AnyRouter | null = null;

export function setRouter(r: AnyRouter): void {
  routerRef = r;
}

function r(): AnyRouter {
  if (!routerRef) throw new Error('Router not ready');
  return routerRef;
}

export function currentProjectSplat(): string | null {
  const m = r().state.matches.find((x) => x.routeId === '/project/$');
  const splat = (m?.params as { _splat?: string } | undefined)?._splat;
  return splat ?? null;
}

export function currentProjectPath(): string | null {
  const s = currentProjectSplat();
  return s ? splatToPath(s) : null;
}

export function currentSearch(): ProjectSearch {
  const m = r().state.matches.find((x) => x.routeId === '/project/$');
  return (m?.search as ProjectSearch | undefined) ?? {};
}

export function setSearch(patch: Partial<ProjectSearch>, replace = false): void {
  const splat = currentProjectSplat();
  if (splat === null) return;
  void r().navigate({
    to: '/project/$',
    params: { _splat: splat },
    search: (prev: ProjectSearch) => clean({ ...prev, ...patch }),
    replace,
  });
}

export function goProject(path: string, search: Partial<ProjectSearch> = {}, replace = false): Promise<void> {
  const file = search.file ?? (isSourceFile(path) ? path : undefined);
  return r().navigate({
    to: '/project/$',
    params: { _splat: pathToSplat(path) },
    search: clean({ ...search, file }),
    replace,
  });
}

function clean(s: ProjectSearch): ProjectSearch {
  const out: ProjectSearch = {};
  for (const [k, v] of Object.entries(s)) {
    if (v !== undefined && v !== null && v !== '') (out as Record<string, unknown>)[k] = v;
  }
  return out;
}
