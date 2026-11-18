export class JsonCodec {
  get binary(): boolean {
    return false;
  }

  get mimeType(): string {
    return "application/octet-stream";
  }

  get name(): string {
    return "json";
  }
}
