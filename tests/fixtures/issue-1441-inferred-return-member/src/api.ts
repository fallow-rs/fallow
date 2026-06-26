function makeRes() {
  return { call() { return 1 } }
}

export class Api {
  public readonly Direct = makeRes()
  public readonly ViaFactory = makeRes()
  // Never accessed by any consumer: must stay flagged (non-vacuous control).
  public readonly DeadMember = makeRes()
}
