import { inject } from 'vue'
import { STORE_KEY } from './keys'
export const useStore = () => inject(STORE_KEY)
