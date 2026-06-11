import { step } from "./step";

export class MessageChecks {
  @step("Assert if the sample record matches")
  public async hasExpectedRecord(): Promise<void> {
    console.log("expected record");
  }

  @step("Assert if the sample message exists for ID '{{recordId}}'")
  public async hasMessageForRecordId(recordId: string): Promise<void> {
    console.log(recordId);
  }

  @step("Unused decorated control")
  public async unusedCheck(): Promise<void> {
    console.log("unused");
  }
}
