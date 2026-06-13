import { mapState } from 'pinia'
import { useMapStore } from './stores/s'
export { useSpreadStore, useKeysStore, useDynStore } from './stores/s'
// Options-API mapState called directly with the store factory.
export const computedOptions = { ...mapState(useMapStore, ['g', 'h']) }
