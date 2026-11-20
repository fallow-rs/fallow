const interFonts = {
  regular: "@expo-google-fonts/inter/400Regular/Inter_400Regular.ttf",
} as const;

export default {
  name: "credits",
  slug: "credits",
  plugins: [["expo-font", { fonts: [interFonts.regular, "assets/fonts/Local.ttf"] }]],
};
