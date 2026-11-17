<script module lang="ts">
  import {
    importedButUnused,
    shadowedInInstanceScript,
    usedInInstanceScript,
    usedInMarkup,
    usedInModuleScript,
  } from "./utils";

  export const fromModuleScript = usedInModuleScript();
</script>

<script lang="ts">
  const value = usedInInstanceScript();
  const double = (shadowedInInstanceScript: number): number => shadowedInInstanceScript * 2;
</script>

<p>{value} {usedInMarkup} {double(1)}</p>
