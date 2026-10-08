import { Command, CommandRunner } from 'nest-commander';

@Command({ name: 'seed' })
export class SeedCommand extends CommandRunner {
  async run(): Promise<void> {}

  unusedOption() {
    return 'never called';
  }
}
