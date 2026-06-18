import { execSync } from "node:child_process";
import moment from "moment";
import type { Moment } from "moment";
import "moment/locale/nl";
import tz from "moment-timezone";
import { suppressed } from "./suppressed";
import { devOnly } from "./tooling/dev";

// A literal-only argument: callee capture is argument-blind, so banned-call
// policy still fires here.
execSync("echo hello");
fetch("/api/status");

export const stamp: Moment = moment();
export const zoned = tz;
export const all = [suppressed, devOnly];
