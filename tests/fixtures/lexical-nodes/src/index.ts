import { ColoredTextNode } from './colored-text';
import { CustomParagraphNode } from './custom-paragraph';
import { VideoNode } from './video-node';

// Stands in for `createEditor({ nodes: [...] })`: the node classes are
// referenced (so their exports are reachable) but none of their members are
// accessed here, so member-level analysis runs on each class. The classes are
// NOT re-exported, so the entry-point public-API skip does not apply.
void new VideoNode();
void new CustomParagraphNode();
void new ColoredTextNode();
