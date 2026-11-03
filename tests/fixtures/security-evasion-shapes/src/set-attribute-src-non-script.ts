export const show = (src: string): void => {
  const img = document.createElement("img");
  img.setAttribute("src", src);
  document.body.append(img);
};
