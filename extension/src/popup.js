// The toolbar popup: one button per kind of screenshot.

for (const el of document.querySelectorAll('[data-i18n]')) {
  el.textContent = chrome.i18n.getMessage(el.dataset.i18n);
}

chrome.commands.getAll().then((commands) => {
  for (const { name, shortcut } of commands) {
    const kbd = document.querySelector(`kbd[data-command="${name}"]`);
    if (kbd) kbd.textContent = shortcut || '';
  }
});

for (const button of document.querySelectorAll('button[data-mode]')) {
  button.addEventListener('click', () => {
    chrome.runtime.sendMessage({ type: 'capture', mode: button.dataset.mode });
    // The page has to be visible again for the capture and the selection.
    window.close();
  });
}
