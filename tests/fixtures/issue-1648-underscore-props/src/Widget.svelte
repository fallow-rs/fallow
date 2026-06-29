<script lang="ts">
  interface Props {
    stage: string;
    variant?: string;
  }

  // Both props are accepted for public-API stability but intentionally unused
  // internally. `stage` is aliased with a leading underscore (the convention
  // honored by `unusedComponentProps.ignorePattern`); `variant` is not.
  let { stage: _stage, variant }: Props = $props();
</script>

<div>static content</div>
