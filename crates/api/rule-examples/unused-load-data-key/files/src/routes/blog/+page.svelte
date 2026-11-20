<script lang="ts">
  let { data } = $props();
</script>

<ul>
  {#each data.posts as post}
    <li>{post.title}</li>
  {/each}
</ul>
