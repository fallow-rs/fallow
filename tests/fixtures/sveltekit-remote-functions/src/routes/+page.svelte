<script lang="ts">
  import { getData } from './data.remote';

  const data = getData();
</script>

<h1>Count: {data.count}</h1>
