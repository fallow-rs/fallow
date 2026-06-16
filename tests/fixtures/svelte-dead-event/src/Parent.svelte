<script>
  import Child from './Child.svelte'
  import Middle from './Middle.svelte'

  function handleSave(event) {
    console.log('saved', event.detail)
  }
</script>

<!-- Parent listens for the child's `save` event with a handler. -->
<Child on:save={handleSave} />

<!-- And renders the forwarding Middle component (which forwards `save`). -->
<Middle on:save={handleSave} />

<!-- A DOM on:click on a native element is not a custom-event listen. -->
<button on:click={() => handleSave(new CustomEvent('click'))}>click</button>
