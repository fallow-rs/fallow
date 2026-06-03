import { DurationMS, type DurationI } from './duration';

const main = (dur: DurationI): number => dur.toMs() + dur.toSec();

main(new DurationMS(1000));
