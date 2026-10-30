// Chrome's offscreen document: decodes images for the service worker, which has no DOM.

/* global decodeInDocument */

chrome.runtime.onMessage.addListener((message, _sender, reply) => {
  if (message.target !== 'offscreen' || message.type !== 'decode') return false;
  decodeInDocument(message.dataUrl, message.mime).then(
    (dataUrl) => reply({ dataUrl }),
    (err) => reply({ error: err.message || String(err) }),
  );
  return true;
});
