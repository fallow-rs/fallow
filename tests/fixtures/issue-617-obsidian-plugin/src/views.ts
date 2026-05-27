import { ItemView, Plugin as ObsidianPlugin, View } from 'obsidian';

export class TerminalItemView extends ItemView {
  getViewType(): string {
    return 'terminal';
  }

  getDisplayText(): string {
    return 'Terminal';
  }

  getIcon(): string {
    return 'terminal';
  }

  onOpen(): void {}

  onClose(): void {}

  onPaneMenu(): void {}

  viewHelper(): void {}
}

export class TerminalView extends View {
  getViewType(): string {
    return 'terminal-view';
  }

  getDisplayText(): string {
    return 'Terminal view';
  }

  onOpen(): void {}

  onClose(): void {}

  viewHelper(): void {}
}

export class PlainObject {
  onload(): void {}

  onOpen(): void {}
}

export class AliasPlugin extends ObsidianPlugin {
  onload(): void {}
}

export class LocalPluginBase extends ObsidianPlugin {}

export class DerivedProjectPlugin extends LocalPluginBase {
  onunload(): void {}
}
