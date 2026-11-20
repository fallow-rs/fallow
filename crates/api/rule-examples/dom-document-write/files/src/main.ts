const showOrderBanner = (): void => {
  document.write(location.hash.slice(1));
};

showOrderBanner();
