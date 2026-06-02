// Positive: local helpers named like sanitizers have no package provenance.
const DOMPurifyLike = {
  sanitize(value: string): string {
    return value;
  },
};

const sanitize = (value: string): string => value;

export function renderNearMisses(el: HTMLElement, userInput: string): void {
  el.innerHTML = DOMPurifyLike.sanitize(userInput);
  document.write(sanitize(userInput));
}
