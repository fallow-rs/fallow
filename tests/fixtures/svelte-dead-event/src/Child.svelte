<script>
  import { createEventDispatcher } from 'svelte'

  const dispatch = createEventDispatcher()

  function onSave() {
    // `save` is listened by a parent via <Child on:save>, so NOT flagged.
    dispatch('save', { ok: true })
  }

  function onDead() {
    // `dead` is listened to nowhere in the project, so it IS flagged.
    dispatch('dead')
  }
</script>

<!-- A DOM on:click is a native event, not a custom dispatched event. -->
<button on:click={onSave}>save</button>
<button on:click={onDead}>dead</button>
