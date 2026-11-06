export const load = (name: string) => import(`./icons/__${name}__`)
export const loadPage = (name: string) => import(`./icons/${name}/index`)
