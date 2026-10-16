export const pages = import.meta.glob([
  '../components/*.js',
  '!../components/skip.js',
  '!**/draft.js',
]);

export const loadLocale = (locale) => import(`../locales/${locale}.js`);
export const loadView = (name) => import('../views/' + name + '.js');
export const widgets = require.context('../widgets', false, /\.js$/);
