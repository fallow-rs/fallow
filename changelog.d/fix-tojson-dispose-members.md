### Fixed

- **Unused class member checks credit `toJSON` and paired `dispose` methods.** `JSON.stringify` calls `toJSON`, so fallow no longer reports it. A `dispose` method is used when its class also declares `[Symbol.dispose]` or `[Symbol.asyncDispose]`. Other `dispose` methods still report, and `usedClassMembers` stays the opt-in for them. Fallow rebuilds the parse and graph caches once to record the pairs.
