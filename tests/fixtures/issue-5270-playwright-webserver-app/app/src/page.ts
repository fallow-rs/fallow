import { formatLabel } from '../../src/format';

export const renderPage = (): void => {
  document.body.textContent = formatLabel('email');
};
