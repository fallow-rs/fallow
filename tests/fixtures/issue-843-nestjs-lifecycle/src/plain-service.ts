// An ordinary class that implements NONE of the Nest interfaces. It declares a
// method with a Nest lifecycle hook name (`onModuleInit`). Because the heritage
// scoping requires `implements OnModuleInit`, this must STILL be reported as
// unused-class-member, proving the lifecycle rules are `implements`-scoped and
// not global. (`onModuleInit` is not a built-in Angular lifecycle name, so the
// only thing that could credit it here is the heritage-scoped Nest rule.)
export class PlainService {
  onModuleInit(): void {
    // Not actually a Nest provider; no `implements OnModuleInit`.
  }
}
