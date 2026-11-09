export class Service {
  private constructor(private readonly items: string[]) {}

  static async create(): Promise<Service> {
    const service = new Service(['a']);
    await Promise.resolve();
    return service;
  }

  static build(): Service {
    return new Service(['b']);
  }

  viaAwait(): string[] {
    return this.items;
  }

  viaSync(): string[] {
    return this.items;
  }

  unused(): string[] {
    return this.items;
  }
}
