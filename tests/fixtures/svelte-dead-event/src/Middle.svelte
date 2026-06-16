<script>
  import Child from './Child.svelte'
</script>

<!-- Event forwarding: <Child on:save> with no handler re-dispatches `save`
     upward. The bare `on:save` counts as a listen for the project-wide set. -->
<Child on:save />
