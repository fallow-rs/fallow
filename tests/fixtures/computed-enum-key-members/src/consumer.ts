import { ProtocolKey } from './flags'

export function readProtocol(target: Record<string, unknown>): unknown {
  return target[ProtocolKey.Protocol]
}

export function readNumeric(target: Record<number, unknown>): unknown {
  return target[ProtocolKey.Numeric]
}
