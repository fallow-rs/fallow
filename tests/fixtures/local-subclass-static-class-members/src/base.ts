export class UrlSyncManager {
  public static calledViaSub(value: string): string {
    return value
  }

  public static passedViaSub(value: string): string {
    return value
  }

  public static passedViaBase(value: string): string {
    return value
  }

  public static arrowViaSub = (value: string): string => value

  public static viaGrandchild(value: string): string {
    return value
  }

  public static viaDeepChain(value: string): string {
    return value
  }

  public static shadowedOnSub(value: string): string {
    return value
  }

  public static viaMixin(value: string): string {
    return value
  }

  public static viaNamespaceBase(value: string): string {
    return value
  }

  public static trulyDead(value: string): string {
    return value
  }

  public instanceViaSub(): number {
    return 1
  }

  public instanceTrulyDead(): number {
    return 2
  }
}

export function mixin<T>(target: T): T {
  return target
}
