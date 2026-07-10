export class RESTApi {
  public viaSimpleBinding = 1
  public viaChainedCall = 2
  public viaChainedCallThenCall = () => 3
  public viaDestructure = 4
  public viaRenamedKey = 5
  public viaDefaultedKey = 6
  public viaNestedKey = { inner: 7 }
  public viaChainedThenDeep = { deep: 8 }
  public viaOptionalChain = 9
  public trulyDead = 10
}
