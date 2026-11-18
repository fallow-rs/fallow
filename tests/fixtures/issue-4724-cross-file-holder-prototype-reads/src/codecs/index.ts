import { JsonCodec } from "./json";
import { MsgpackCodec } from "./msgpack";

const codecs = { json: JsonCodec, msgpack: MsgpackCodec };

export default codecs;
