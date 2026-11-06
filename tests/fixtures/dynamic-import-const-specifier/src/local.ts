export const loadLocal = () => {
  const target = './target'
  return import(target)
}
