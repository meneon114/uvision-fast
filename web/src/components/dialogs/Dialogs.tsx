import { AboutDialog, ShortcutsDialog } from './HelpDialogs';
import { AddItemDialog, NewProjectDialog } from './ProjectDialogs';
import { ExamplesDialog, WorkspaceDialog } from './WorkspaceDialogs';
import { ExportDialog, FlashDialog, PacksDialog } from './FlashDialogs';
import { OptionsDialog } from './OptionsDialog';
import { PaletteDialog } from './PaletteDialog';
import { PromptDialog } from './PromptDialog';

export function Dialogs() {
  return (
    <>
      <PaletteDialog />
      <OptionsDialog />
      <NewProjectDialog />
      <AddItemDialog />
      <WorkspaceDialog />
      <ExamplesDialog />
      <ExportDialog />
      <FlashDialog />
      <PacksDialog />
      <ShortcutsDialog />
      <AboutDialog />
      <PromptDialog />
    </>
  );
}
