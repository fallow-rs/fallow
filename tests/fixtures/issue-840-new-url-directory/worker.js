// Worker script referenced via new URL('./worker.js', import.meta.url).
self.onmessage = (event) => {
    self.postMessage(event.data);
};
