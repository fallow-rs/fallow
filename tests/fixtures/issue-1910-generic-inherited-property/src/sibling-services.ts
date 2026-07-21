export class UsedSiblingClient {
  shared(): string {
    return "used";
  }
}

export class UnusedSiblingClient {
  shared(): string {
    return "unused";
  }
}

export class CallingSiblingService {
  constructor(public readonly client: UsedSiblingClient) {}

  run(): string {
    return this.client.shared();
  }
}

export class SilentSiblingService {
  constructor(public readonly client: UnusedSiblingClient) {}

  keepAlive(): string {
    return "alive";
  }
}
