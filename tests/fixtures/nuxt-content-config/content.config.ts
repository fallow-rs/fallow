export default defineContentConfig({
  collections: {
    docs: defineCollection({ type: 'page', source: '**/*.md' })
  }
});
