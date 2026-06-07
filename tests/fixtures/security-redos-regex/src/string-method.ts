export const stringMethod = (req: { params: { slug: string } }): RegExpMatchArray | null => {
  return req.params.slug.match(/^(a|aa)+$/);
};
