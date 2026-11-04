// A local `shell` object, without the shelljs import.
const shell = {
  exec(command: string): string {
    return command;
  },
};

export function run(command: string): string {
  return shell.exec(command);
}
