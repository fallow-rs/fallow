import { Room, ChatRoom, AliasRoom } from "./room";
import { Api } from "./api";
import { Pipeline } from "./workflow";
import { FakeRoom, OtherRoom, PlainRoom } from "./negative";

export const classes = [Room, ChatRoom, AliasRoom, Api, Pipeline, FakeRoom, OtherRoom, PlainRoom];
