const { spawn } = require("node:child_process");
const { writeFileSync } = require("node:fs");

const [, , mode, ...args] = process.argv;

switch (mode) {
  case "normal": {
    let input = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => {
      input += chunk;
    });
    process.stdin.on("end", () => {
      process.stdout.write(`stdout:${input}`);
      process.stderr.write("stderr:normal");
    });
    break;
  }
  case "nonzero":
    process.stderr.write("child failed\nsecondary detail\n");
    process.exitCode = 7;
    break;
  case "stdout":
    process.stdout.write("x".repeat(Number(args[0])));
    break;
  case "stderr":
    process.stderr.write("y".repeat(Number(args[0])));
    break;
  case "hang":
    writeFileSync(args[0], String(process.pid));
    setInterval(() => {}, 1_000);
    break;
  case "tree": {
    const descendant = spawn(process.execPath, [__filename, "descendant", args[1]], {
      stdio: "ignore",
    });
    writeFileSync(args[0], String(descendant.pid));
    setInterval(() => {}, 1_000);
    break;
  }
  case "descendant":
    process.on("SIGTERM", () => {
      writeFileSync(args[0], "terminated");
      process.exit(0);
    });
    setInterval(() => {}, 1_000);
    break;
  default:
    process.stderr.write(`unknown mode: ${mode}`);
    process.exitCode = 2;
}
