import Card from "./lib/Card.svelte";
import "./lib/Banner.vue";
import { default as Panel } from "./lib/Panel.svelte";
import * as Dialog from "./lib/Dialog.vue";

export const mount = (): unknown[] => [Card, Panel, Dialog];
