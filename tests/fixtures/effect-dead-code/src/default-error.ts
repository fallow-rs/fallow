import { Data } from "effect"

class ViaDefault extends Data.TaggedError("ViaDefault")<{}> {}

export default ViaDefault
