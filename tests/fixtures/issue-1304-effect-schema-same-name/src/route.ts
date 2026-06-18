import {
  AssistantPromptResponse,
  BlockScopedParentSchema,
  HoistedShadowParentSchema,
} from "./schema";

export const route = {
  blockScoped: BlockScopedParentSchema,
  hoistedShadow: HoistedShadowParentSchema,
  response: AssistantPromptResponse,
};
