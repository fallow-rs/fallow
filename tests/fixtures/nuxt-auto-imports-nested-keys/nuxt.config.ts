export default defineNuxtConfig({
  routeRules: {
    '/api/imports': { prerender: true },
    '/docs/components': { prerender: true },
  },
})
