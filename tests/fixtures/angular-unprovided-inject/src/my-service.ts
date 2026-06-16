// A plain class token. `inject(MyService)` is OUT of scope for
// unprovided-inject: a class token self-provides via providedIn / third-party
// provideX(), so flagging it would be false-positive-prone. Only user
// InjectionToken symbols are eligible.
export class MyService {
  greet(): string {
    return 'hi';
  }
}
