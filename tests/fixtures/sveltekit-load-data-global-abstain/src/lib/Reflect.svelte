<script lang="ts">
  import { page } from '$app/state'

  // Reflective whole-object read of the page-data store: any route's key could
  // be consumed here, so the detector abstains project-wide.
  const allKeys = Object.values(page.data)
</script>

<pre>{allKeys.length}</pre>
