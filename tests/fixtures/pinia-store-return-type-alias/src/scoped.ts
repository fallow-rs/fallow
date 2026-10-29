import { useCounterStore } from './store'

// The alias is declared inside the function body that uses it.
export function readScoped() {
  type Local = ReturnType<typeof useCounterStore>
  const read = (store: Local) => store.scopedTotal

  // An inner alias of the same name is another type, so this read must not
  // credit the store member `deadFlag` (control).
  function readShadowed() {
    type Local = { deadFlag: boolean }
    const readFlag = (value: Local) => value.deadFlag
    return readFlag({ deadFlag: false })
  }

  return [read(useCounterStore()), readShadowed()]
}
