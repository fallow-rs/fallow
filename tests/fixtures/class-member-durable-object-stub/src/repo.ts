export class Repo {
  get(key: string): { unusedOnRepo(): boolean } {
    return { unusedOnRepo: () => key.length > 0 };
  }

  unusedOnRepo(): boolean {
    return false;
  }
}
