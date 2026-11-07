import { Table } from "./table.js";

// Each `all` below holds another value than the top-level function, so its
// calls must not credit `Table.reset`.
export function all(): Table[] {
  return [new Table()];
}

export function retry(): void {
  try {
    all();
  } catch (all: any) {
    all()[0].reset();
  }
}

export namespace Other {
  function all(): { reset(): void }[] {
    return [];
  }

  export const run = (): void => {
    for (const item of all()) {
      item.reset();
    }
  };
}

namespace Local {
  function all(): { reset(): void }[] {
    return [];
  }

  all()[0].reset();

  export namespace Inner {
    function all(): { reset(): void }[] {
      return [];
    }

    all()[0].reset();
  }
}
