<script>
  const key = "dynamic";
  let { computed, [key]: dynamic } = $props();

  console.log(dynamic);
</script>

<p>Computed abstain component</p>
