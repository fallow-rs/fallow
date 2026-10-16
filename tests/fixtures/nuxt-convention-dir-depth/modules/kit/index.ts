import { addPlugin, createResolver, defineNuxtModule } from '@nuxt/kit'

export default defineNuxtModule({
  setup() {
    const resolver = createResolver(import.meta.url)
    addPlugin({ src: resolver.resolve('./runtime/plugin') })
  },
})
