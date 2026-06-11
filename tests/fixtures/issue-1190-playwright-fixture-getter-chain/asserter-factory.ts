import { MessageChecks } from "./message-checks";

export class AsserterFactory {
  private _messageChecks?: MessageChecks;

  public get messageChecks(): MessageChecks {
    this._messageChecks ??= new MessageChecks();
    return this._messageChecks;
  }
}
