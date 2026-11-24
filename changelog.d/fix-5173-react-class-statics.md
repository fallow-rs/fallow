### Fixed

- **React component static properties (`defaultProps`, `displayName`, `propTypes`, `contextTypes`, and `childContextTypes`) are no longer reported as unused class members.** React reads these statics from component classes at runtime alongside `contextType`, `getDerivedStateFromProps`, and `getDerivedStateFromError`.
  (Closes [#5173](https://github.com/fallow-rs/fallow/issues/5173))
