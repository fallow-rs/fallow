declare const Worker: new (name: string, handler: (job: { data: unknown }) => void) => unknown;

new Worker("email", ({ data }) => {
  eval(data);
});
