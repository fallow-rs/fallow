import { provide } from 'vue'
import { GLOBAL_KEY } from './globalKeys'
import { setup as p } from './provider'
import { setup as c } from './consumer'
import { setup as g } from './globalConsumer'
const app: { provide: typeof provide } = { provide }
app.provide(GLOBAL_KEY, 1)
p(); c(); g()
