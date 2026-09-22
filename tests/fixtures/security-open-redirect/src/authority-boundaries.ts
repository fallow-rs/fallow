const ORIGIN = "https://app.example.com";
type Reply = { redirect: (destination: string) => void };

export const hostSuffix = (reply: Reply, input: string): void => reply.redirect(`${ORIGIN}${input}`);
export const rootSlash = (reply: Reply, input: string): void => reply.redirect(`/${input}`);
export const fixedPath = (reply: Reply, input: string): void => reply.redirect(ORIGIN + "/path/" + input);
export const relativePath = (reply: Reply, input: string): void => reply.redirect(`/path/${input}`);
export const assignedHost = (input: string): void => { window.location.href = `${ORIGIN}${input}`; };
export const assignedPath = (input: string): void => { window.location.href = `${ORIGIN}/path/${input}`; };
