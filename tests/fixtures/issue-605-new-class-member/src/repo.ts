// Direct constructor-receiver shape (everr#144):
// `new TracesRepository(client).search(data)`.
export class TracesRepository {
  constructor(private readonly client: unknown) {}

  search(input: string): string {
    return `${input}:${String(this.client)}`;
  }

  getTrace(id: string): string {
    return id;
  }

  // Genuinely unused method on the same class: must STILL be reported.
  // Proves the fix does not blanket-credit every member once one direct
  // call on a constructed instance is observed.
  unusedRepoMethod(): void {}
}
