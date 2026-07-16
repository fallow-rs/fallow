import type { AppState } from "./state";
import { renderTreemap } from "./treemap";

const performSearch = (state: AppState, query: string): void => {
  state.searchQuery = query;
  state.searchResults.clear();

  if (query === "") {
    renderTreemap(state);
    return;
  }

  const lowerQuery = query.toLowerCase();

  // Find matching layout nodes
  for (let i = 0; i < state.layout.length; i++) {
    const node = state.layout[i].node;
    if (node.name.toLowerCase().includes(lowerQuery) ||
        node.path.toLowerCase().includes(lowerQuery)) {
      state.searchResults.add(i);
    }
  }

  renderTreemap(state);
};

export const setupSearch = (state: AppState): HTMLInputElement => {
  const input = document.createElement("input");
  input.type = "text";
  input.id = "fallow-search";
  input.placeholder = "Search files… (/)";
  input.setAttribute("autocomplete", "off");
  input.setAttribute("aria-label", "Search files");

  let debounceTimer: ReturnType<typeof setTimeout>;
  input.addEventListener("input", () => {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      performSearch(state, input.value);
    }, 150);
  });

  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      input.value = "";
      performSearch(state, "");
      input.blur();
    }
  });

  return input;
};
