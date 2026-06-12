import { MessageChecks } from "./message-checks";

export type AppFixture = {
  assert: {
    messageChecks: MessageChecks;
  };
};

export class FixtureOrchestrator {
  public createApp(): AppFixture {
    return {
      assert: {
        messageChecks: new MessageChecks(),
      },
    };
  }
}
