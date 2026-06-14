<template>
  <img src="./hero.png" alt="hero" />
  <img src="./missing.png" alt="gone" />
  <img :src="dynamicSrc" alt="dynamic" />
  <MyIcon src="./also-not-an-asset.png" />
</template>
<script setup lang="ts">
import MyIcon from "./MyIcon.vue"
const dynamicSrc = "./whatever.png"
</script>
