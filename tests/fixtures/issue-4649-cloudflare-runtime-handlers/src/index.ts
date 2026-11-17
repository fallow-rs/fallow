import { Room, ChatRoom, AliasRoom } from "./room";
import { Api } from "./api";
import { Pipeline } from "./workflow";
import { FakeRoom, OtherRoom, PlainRoom } from "./negative";
import { NsRoom, NsUnknownBase, OtherNsRoom } from "./namespace";
import DefaultChainRoom, { GenericChainRoom, LocalChainRoom, RenamedRoom } from "./local-chain";
import { CrossFileRoom } from "./cross-file";
import { ProtectedRoom } from "./protected";
import { SpecifierRoom } from "./specifier";
import DefaultWorkflow from "./default-workflow";

export const classes = [
  Room,
  ChatRoom,
  AliasRoom,
  Api,
  Pipeline,
  FakeRoom,
  OtherRoom,
  PlainRoom,
  NsRoom,
  NsUnknownBase,
  OtherNsRoom,
  LocalChainRoom,
  GenericChainRoom,
  RenamedRoom,
  DefaultChainRoom,
  CrossFileRoom,
  ProtectedRoom,
  SpecifierRoom,
  DefaultWorkflow,
];
