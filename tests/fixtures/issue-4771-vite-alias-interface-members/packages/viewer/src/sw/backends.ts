import type { Backend } from "@iso/trace/loader";

export class ZipBackend implements Backend {
  isLive(): boolean {
    return false;
  }

  readText(name: string): string {
    return name;
  }

  unusedZipHelper(): number {
    return 1;
  }
}

export class FetchBackend implements Backend {
  isLive(): boolean {
    return true;
  }

  readText(name: string): string {
    return name.toUpperCase();
  }
}
