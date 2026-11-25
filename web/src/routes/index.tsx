import { createFileRoute } from '@tanstack/react-router';
import { Workbench } from '../components/shell/Workbench';

export const Route = createFileRoute('/')({
  component: () => <Workbench />,
});
