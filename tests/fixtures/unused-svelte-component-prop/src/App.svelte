<script>
  import ComputedAbstain from "./ComputedAbstain.svelte";
  import DeadProp from "./DeadProp.svelte";
  import RestAbstain from "./RestAbstain.svelte";
  import ScriptUsed from "./ScriptUsed.svelte";
  import TemplateUsed from "./TemplateUsed.svelte";
</script>

<DeadProp deadProp="unused" />
<ScriptUsed usedScript="script" />
<TemplateUsed shown="template" />
<RestAbstain forwarded="forwarded" other="opaque" />
<ComputedAbstain computed="computed" />
