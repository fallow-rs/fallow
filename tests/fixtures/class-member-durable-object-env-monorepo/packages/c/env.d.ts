// The import declaration makes this file a module, so its `Env` is not the
// global `Env`.
import type { RoomC } from './src/room';

interface Env {
  ROOM: DurableObjectNamespace<RoomC>;
}
