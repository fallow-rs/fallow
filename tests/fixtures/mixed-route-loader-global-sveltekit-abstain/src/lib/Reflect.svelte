<script lang="ts">
  import { page } from '$app/stores'

  const allKeys = Object.values($page.data)
</script>

<pre>{allKeys.length}</pre>
