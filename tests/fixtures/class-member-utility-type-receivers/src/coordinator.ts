export class Coordinator {
  viaParamProperty(): number {
    return 1;
  }

  viaField(): number {
    return 2;
  }

  viaCtorParam(): number {
    return 3;
  }

  viaInterfaceField(): number {
    return 4;
  }

  viaArrowParam(): number {
    return 5;
  }

  viaPartial(): number {
    return 6;
  }

  viaReadonlyOmit(): number {
    return 7;
  }

  unused(): number {
    return 8;
  }
}
