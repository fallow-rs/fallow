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

  @step("Assert if the merged sample record matches")
  public async hasMergedRecord(): Promise<void> {
    console.log("merged record");
  }

  @step("Assert if the merged sample message exists for ID '{{recordId}}'")
  public async hasMergedMessageForRecordId(recordId: string): Promise<void> {
    console.log(recordId);
  }

  @step("Assert if the extended sample record matches")
  public async hasExtendedRecord(): Promise<void> {
    console.log("extended record");
  }

  @step("Assert if the extended sample message exists for ID '{{recordId}}'")
  public async hasExtendedMessageForRecordId(recordId: string): Promise<void> {
    console.log(recordId);
  }

  @step("Assert if the extended merged sample record matches")
  public async hasExtendedMergedRecord(): Promise<void> {
    console.log("extended merged record");
  }

  @step("Unused decorated control")
  public async isActuallyUnused(): Promise<void> {
    console.log("unused");
  }

  @step("Second unused decorated control")
  public async isActuallyUnusedExtended(): Promise<void> {
    console.log("unused extended");
  }
}
