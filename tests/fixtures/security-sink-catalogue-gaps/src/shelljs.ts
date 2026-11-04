import shell from "shelljs";

export function run(dir: string): void {
  shell.exec("ls " + dir);
}
