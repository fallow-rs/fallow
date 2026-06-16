<script>
  import { createEventDispatcher } from 'svelte'

  export let name = 'gone'

  const dispatch = createEventDispatcher()

  function fire() {
    // A dynamic dispatch(<nonLiteral>) makes the event name unknowable, so the
    // whole component abstains: `gone` is NOT flagged even though no one listens.
    dispatch(name)
  }
</script>

<button on:click={fire}>fire</button>
