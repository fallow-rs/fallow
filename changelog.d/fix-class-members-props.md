### Fixed

- **An unused overloaded method is one `unused_class_members` entry.** Fallow reported one entry for each overload signature and one for the implementation of the method. It now reports one entry at the line of the implementation. A method with bodyless signatures only, for example an abstract method with overloads, gives one entry at its first signature.
  (Closes [#4735](https://github.com/fallow-rs/fallow/issues/4735))
