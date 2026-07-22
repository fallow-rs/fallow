<script setup lang="ts" generic="T extends Record<string, unknown>">
import { ref } from 'vue';
const items = ref<T[]>([]);
</script>
