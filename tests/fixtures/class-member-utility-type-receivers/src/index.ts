import { Coordinator } from './coordinator';

class ParamPropertyHolder {
  constructor(private readonly coord: Pick<Coordinator, 'viaParamProperty'>) {}

  run(): number {
    return this.coord.viaParamProperty();
  }
}

class FieldHolder {
  private coord: Pick<Coordinator, 'viaField'>;

  constructor(source: Coordinator) {
    this.coord = source;
  }

  run(): number {
    return this.coord.viaField();
  }
}

class CtorParamHolder {
  readonly total: number;

  constructor(coord: Pick<Coordinator, 'viaCtorParam'>) {
    this.total = coord.viaCtorParam();
  }
}

interface TrackerOptions {
  coord: Pick<Coordinator, 'viaInterfaceField'>;
}

const track = (opts: TrackerOptions): number => opts.coord.viaInterfaceField();

const viaArrow = (coord: Pick<Coordinator, 'viaArrowParam'>): number => coord.viaArrowParam();

const viaPartial = (coord: Partial<Coordinator>): number => coord.viaPartial?.() ?? 0;

const viaReadonlyOmit = (coord: Readonly<Omit<Coordinator, 'unused'>>): number =>
  coord.viaReadonlyOmit();

const holder: { source: Coordinator } = { source: new Coordinator() };

export const main = (): number =>
  new ParamPropertyHolder(holder.source).run() +
  new FieldHolder(holder.source).run() +
  new CtorParamHolder(holder.source).total +
  track({ coord: holder.source }) +
  viaArrow(holder.source) +
  viaPartial(holder.source) +
  viaReadonlyOmit(holder.source);
