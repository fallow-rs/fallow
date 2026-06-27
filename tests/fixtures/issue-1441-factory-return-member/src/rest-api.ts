// Internal class (exported from a non-entry module, never re-exported by the
// entry, so NOT public API). Its members are subject to unused-class-member
// detection: `Plan` is consumed only through the factory in consumer.ts, while
// `unusedMethod` is genuinely dead.
export class RESTApi {
  Plan(): number {
    return 1
  }

  unusedMethod(): number {
    return 2
  }
}
