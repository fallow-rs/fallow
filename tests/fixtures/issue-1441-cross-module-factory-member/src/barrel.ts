// Re-export barrel: the consumer imports `makeDirect` through here, exercising
// the re-export origin walk in the cross-module factory-fn credit. Issue #1441.
export { makeDirect } from './composables'
