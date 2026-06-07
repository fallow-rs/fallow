declare const app: {
  post(path: string, handler: (req: unknown) => void): void;
};
app.post("/run", (req) => {
  eval(req);
  eval(req.body);
});
