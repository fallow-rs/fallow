export class Lock {
  [Symbol.dispose]() {}

  dispose() {}
}

export class AsyncLock {
  async [Symbol.asyncDispose]() {}

  dispose() {}
}

export class Handle {
  dispose() {}
}
