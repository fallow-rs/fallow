const modules = import.meta.glob('./modules/**/!(*.spec).js', { eager: true });
const locales = import.meta.glob('./locales/@(en|nl).js');

export default { modules, locales };
