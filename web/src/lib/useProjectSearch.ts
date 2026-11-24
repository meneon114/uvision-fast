import { useSearch } from '@tanstack/react-router';
import type { ProjectSearch } from './search';

// Search params of the project route, or an empty object on other routes.
export function useProjectSearch(): ProjectSearch {
  return useSearch({ strict: false }) as ProjectSearch;
}
