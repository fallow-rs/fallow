declare const app: {
  post(path: string, handler: (req: unknown) => void): void;
};
declare const cache: {
  get(key: string, handler: (value: string) => void): void;
};
app.post("/run", (req) => {
  eval(req);
  eval(req.body);
});
cache.get("token", (value) => {
  /^(a+)+$/.test(value);
});
