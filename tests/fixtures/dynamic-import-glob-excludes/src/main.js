const pages = import.meta.glob(['./pages/*.js', '!./pages/skip.js', '!**/draft.js']);

export const loadView = (id) => import(`./[id]/${id}.js`);
export const loadItem = (name) => import('./{group}/' + name + '.js');

export default pages;
