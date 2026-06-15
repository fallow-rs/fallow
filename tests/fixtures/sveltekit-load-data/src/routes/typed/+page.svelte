<script lang="ts">
  import Widget from '$lib/Widget.svelte'
  import type { PageData } from './$types'

  export let data: PageData
</script>

<Widget value={data.shown} />
