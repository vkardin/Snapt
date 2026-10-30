// The background script: the image context menu and saving images as PNG or JPG.
// Chrome runs it as a service worker, Firefox as an event page.

/* global convertImage, decodeInDocument, blobToDataUrl */

if (typeof importScripts === 'function') {
  importScripts('convert.js');
}

const FORMATS = {
  'save-png': { type: 'image/png', ext: 'png' },
  'save-jpg': { type: 'image/jpeg', ext: 'jpg' },
};

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: 'save-png',
      title: chrome.i18n.getMessage('savePng'),
      contexts: ['image'],
    });
    chrome.contextMenus.create({
      id: 'save-jpg',
      title: chrome.i18n.getMessage('saveJpg'),
      contexts: ['image'],
    });
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  const format = FORMATS[info.menuItemId];
  if (!format || !info.srcUrl) return;
  saveImage(info.srcUrl, format, tab, info.frameId ?? 0).catch((err) => {
    console.error(err);
    if (tab?.id >= 0) {
      const text = chrome.i18n.getMessage('saveFailed', [err.message || String(err)]);
      chrome.tabs.sendMessage(tab.id, { type: 'toast', text }, { frameId: 0 }).catch(() => {});
    }
  });
});

async function saveImage(src, format, tab, frameId) {
  const blob = await fetchImage(src, tab, frameId);
  const converted = await convertImage(blob, format.type, decodeElsewhere);
  await download(converted, `${baseName(src)}.${format.ext}`);
}

// The extension may read any URL, so most images are fetched here, free of the
// page's CORS rules. Blob URLs belong to the page, which has to read them itself,
// and the page is also the fallback for images that need its cookies or referrer.
async function fetchImage(src, tab, frameId) {
  if (!src.startsWith('blob:')) {
    try {
      const response = await fetch(src, { credentials: 'include' });
      if (response.ok) return await response.blob();
    } catch {
      // Try the page below.
    }
  }
  if (!(tab?.id >= 0)) throw new Error('the image is not reachable');
  const reply = await chrome.tabs.sendMessage(tab.id, { type: 'fetch', url: src }, { frameId });
  if (!reply || reply.error) throw new Error(reply?.error || 'the page did not answer');
  return (await fetch(reply.dataUrl)).blob();
}

// Decodes SVG and other images a worker can't: Firefox's event page has a DOM of
// its own, Chrome's service worker borrows one from an offscreen document.
async function decodeElsewhere(dataUrl, type) {
  const converted = typeof document === 'undefined'
    ? await decodeOffscreen(dataUrl, type)
    : await decodeInDocument(dataUrl, type);
  return (await fetch(converted)).blob();
}

let offscreenReady;

async function decodeOffscreen(dataUrl, type) {
  offscreenReady ??= (async () => {
    const contexts = await chrome.runtime.getContexts({ contextTypes: ['OFFSCREEN_DOCUMENT'] });
    if (contexts.length === 0) {
      await chrome.offscreen.createDocument({
        url: 'offscreen.html',
        reasons: ['BLOBS'],
        justification: 'Decode SVG images to save them as PNG or JPG.',
      });
    }
  })();
  try {
    await offscreenReady;
  } finally {
    offscreenReady = undefined;
  }
  const reply = await chrome.runtime.sendMessage({ target: 'offscreen', type: 'decode', dataUrl, mime: type });
  if (!reply || reply.error) throw new Error(reply?.error || 'the image could not be decoded');
  return reply.dataUrl;
}

// Service workers have no URL.createObjectURL, so Chrome downloads a data URL.
async function download(blob, filename) {
  const blobUrl = typeof URL.createObjectURL === 'function' ? URL.createObjectURL(blob) : null;
  const url = blobUrl ?? (await blobToDataUrl(blob));
  try {
    await chrome.downloads.download({ url, filename, saveAs: true });
  } finally {
    if (blobUrl) setTimeout(() => URL.revokeObjectURL(blobUrl), 60_000);
  }
}

// The file name in an image URL without its extension, safe on every system.
function baseName(src) {
  let name = '';
  try {
    const url = new URL(src);
    if (url.protocol === 'http:' || url.protocol === 'https:') {
      name = decodeURIComponent(url.pathname.split('/').pop() || '');
    }
  } catch {
    // Keep the default name.
  }
  name = name
    .replace(/\.[a-z0-9]{1,5}$/i, '')
    .replace(/[\u0000-\u001f\\/:*?"<>|~]+/g, '_')
    .replace(/^[\s.]+|[\s.]+$/g, '')
    .slice(0, 100);
  return name || 'image';
}
