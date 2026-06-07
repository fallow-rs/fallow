export const constructor = (req: { body: { value: string } }): boolean => {
  const re = new RegExp("^(a+)+$");
  return re.test(req.body.value);
};
