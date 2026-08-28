self.onmessage = event => {
  self.postMessage({
    answer: event.data.left + event.data.right,
    kind: "worker",
  });
};
