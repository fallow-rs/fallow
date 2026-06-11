import { AsserterFactory } from "./asserter-factory";
import { MessageChecks } from "./message-checks";

export type AppFixture = {
  assert: {
    messageChecks: MessageChecks;
  };
};

export class FixtureOrchestrator {
  private readonly asserterFactory = new AsserterFactory();

  public createApp(): AppFixture {
    return {
      assert: {
        messageChecks: this.asserterFactory.messageChecks,
      },
    };
  }
}
