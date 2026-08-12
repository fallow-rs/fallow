// npm's start lifecycle reaches this file through the `serve` script. The
// quoted path and script indirection must preserve production reachability.
import { run } from "runtime-helper";

run();
