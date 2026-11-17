interface __BaseEnv_Env {
  ROOM: DurableObjectNamespace<import('./src/room').RoomB>;
}
interface Env extends __BaseEnv_Env {}
