import { TaskAsserter } from '../../src/task-asserter';

export class TaskAsserterFactory {
  private taskAsserterInstance?: TaskAsserter;

  get taskAsserter(): TaskAsserter {
    return (this.taskAsserterInstance ??= new TaskAsserter());
  }
}
