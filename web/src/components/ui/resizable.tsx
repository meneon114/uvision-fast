import * as R from '@ui-sdk/react/resizable';
import { styled } from './styled';

// Panels lay out their own content.
export const ResizablePanel = styled(R.ResizablePanel, 'p-0! flex flex-col min-w-0 min-h-0', 'ResizablePanel');
export const ResizableHandle = styled(R.ResizableHandle, 'bg-border hover:bg-acc data-dragging:bg-acc', 'ResizableHandle');
export { createResizableController, Resizable } from '@ui-sdk/react/resizable';
