import { DataService } from '@services/data-service';

// Typed locally-instantiated instance via wrapper call (useMemo-style).
// The first array-destructured element is bound to DataService.
const [svc] = useMemo(() => new DataService(), []);
svc.fetchData();

export class App {}

// Simulate useMemo without an actual import to keep the fixture dependency-free.
function useMemo<T>(factory: () => T, _deps: unknown[]): [T] {
    return [factory()];
}
