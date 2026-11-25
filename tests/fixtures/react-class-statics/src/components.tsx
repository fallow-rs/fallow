import React, { Component, PureComponent } from 'react';

export class Greeting extends React.Component {
  static defaultProps = { name: 'world' };
  static displayName = 'Greeting';
  static propTypes = {};
  static contextTypes = {};
  static childContextTypes = {};
  static contextType = null;
  static getDerivedStateFromProps(): null {
    return null;
  }
  static getDerivedStateFromError(): null {
    return null;
  }
  formatName(): string {
    return 'unused';
  }
  render(): null {
    return null;
  }
}

export class Panel extends Component {
  static defaultProps = { open: false };
  static displayName = 'Panel';
  render(): null {
    return null;
  }
}

export class Badge extends PureComponent {
  static defaultProps = { tone: 'info' };
  static displayName = 'Badge';
  render(): null {
    return null;
  }
}

export class UserModel {
  displayName = 'user';
  save(): void {}
}
