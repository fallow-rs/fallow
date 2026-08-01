export class TaskAsserter {
  async hasTaskForReference(reference: string): Promise<void> {
    console.log(reference);
  }

  async unusedAsserterOnly(): Promise<void> {
    console.log('never called');
  }
}
