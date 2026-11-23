### Fixed

- **Vue SFC `<template>` complexity units now report the line count of the template block.** The synthetic `<template>` unit previously received the line count of the entire Vue file, which inflated `vital_signs.functions_over_60_loc_per_k` and caused short templates in long single-file components to be listed in `large_functions`. (Closes [#4992](https://github.com/fallow-rs/fallow/issues/4992))
