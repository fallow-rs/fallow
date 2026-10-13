import { BufferExporter, RateSampler, StampProcessor } from "./trace";
import { HeaderPropagator, PollReader } from "./other";

// Stands in for handing each instance to the SDK. The classes are referenced
// but not re-exported, so member-level analysis runs on them.
void new BufferExporter();
void new RateSampler();
void new StampProcessor();
void new HeaderPropagator();
void new PollReader();
