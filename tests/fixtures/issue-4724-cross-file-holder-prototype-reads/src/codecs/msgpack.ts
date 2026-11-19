export class MsgpackCodec {
  get binary(): boolean {
    return true;
  }

  get mimeType(): string {
    return "application/octet-stream";
  }

  get name(): string {
    return "msgpack";
  }
}
