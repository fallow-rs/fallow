<script>
  export let userHtml = "";
</script>

<main>
  <h1>Profile</h1>
  <p>Unsafe content:</p>
  {@html userHtml}
</main>
