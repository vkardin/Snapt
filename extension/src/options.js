// The settings page: shortcuts, what the extension does, and links.

const t = (key, ...subs) => chrome.i18n.getMessage(key, subs);

document.documentElement.lang = chrome.i18n.getUILanguage();
document.title = t('optTitle');
for (const el of document.querySelectorAll('[data-i18n]')) el.textContent = t(el.dataset.i18n);
document.querySelector('.version').textContent = `v${chrome.runtime.getManifest().version}`;

const names = { area: 'captureArea', visible: 'captureVisible', page: 'capturePage', screen: 'captureScreen' };

chrome.commands.getAll().then((commands) => {
  const list = document.querySelector('.shortcuts');
  const order = Object.keys(names);
  commands = commands.filter((c) => names[c.name]).sort((a, b) => order.indexOf(a.name) - order.indexOf(b.name));
  for (const { name, shortcut } of commands) {
    const li = document.createElement('li');
    const label = document.createElement('span');
    label.textContent = t(names[name]);
    li.append(label);
    if (shortcut) {
      const kbd = document.createElement('kbd');
      kbd.textContent = shortcut;
      li.append(kbd);
    } else {
      const none = document.createElement('small');
      none.className = 'none';
      none.textContent = t('optNotSet');
      li.append(none);
    }
    list.append(li);
  }
});

// Chrome opens its shortcut page in a tab; Firefox 137+ has its own call, older
// versions only get directions.
const isFirefox = typeof browser !== 'undefined' && !!browser.runtime?.getBrowserInfo;
document.querySelector('[data-action="shortcuts"]').addEventListener('click', async () => {
  try {
    if (isFirefox) {
      if (!browser.commands.openShortcutSettings) throw new Error('unsupported');
      await browser.commands.openShortcutSettings();
    } else {
      await chrome.tabs.create({ url: 'chrome://extensions/shortcuts' });
    }
  } catch {
    const hint = document.querySelector('.hint');
    hint.textContent = t('optShortcutsFirefox');
    hint.hidden = false;
  }
});
