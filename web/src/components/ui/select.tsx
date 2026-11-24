import * as S from '@ui-sdk/react/select';
import { POPUP, POPUP_ITEM } from './menu';
import { styled } from './styled';

export const SelectContent = styled(S.SelectContent, POPUP, 'SelectContent');
export const SelectItem = styled(S.SelectItem, POPUP_ITEM, 'SelectItem');
export { Select, SelectItemIndicator, SelectTrigger, SelectValue } from '@ui-sdk/react/select';
