import * as M from '@ui-sdk/react/menu';
import { styled } from './styled';

// Shared by menus and select lists.
export const POPUP = 'min-w-55 p-1 border border-border2 rounded-md bg-bg text-fg2 shadow-pop text-ui';
export const POPUP_ITEM = 'min-h-6 rounded-sm py-0.5 data-highlighted:bg-acc data-highlighted:text-accfg';

export const MenuContent = styled(M.MenuContent, POPUP, 'MenuContent');
export const MenuSubContent = styled(M.MenuSubContent, POPUP, 'MenuSubContent');
export const MenuItem = styled(M.MenuItem, `${POPUP_ITEM} **:data-[slot=menu-shortcut]:text-inherit **:data-[slot=menu-shortcut]:opacity-70`, 'MenuItem');
export const MenuCheckboxItem = styled(M.MenuCheckboxItem, POPUP_ITEM, 'MenuCheckboxItem');
export const MenuRadioItem = styled(M.MenuRadioItem, POPUP_ITEM, 'MenuRadioItem');
// The popup sits inside the menubar, so ui-sdk sizes a submenu trigger like a menubar trigger; set it back to an item's look.
export const MenuSubTrigger = styled(
  M.MenuSubTrigger,
  'h-6.5 min-h-6 px-2 py-0 rounded-sm text-fg2 cursor-pointer data-highlighted:bg-acc data-highlighted:text-accfg',
  'MenuSubTrigger',
);
export const MenuGroupLabel = styled(M.MenuGroupLabel, 'text-muted text-2xs', 'MenuGroupLabel');
export { Menu, MenuGroup, MenuItemIcon, MenuItemIndicator, MenuRadioGroup, MenuSeparator, MenuSub, MenuTrigger } from '@ui-sdk/react/menu';
