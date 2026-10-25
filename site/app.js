(() => {
  const lang = document.documentElement.lang;
  const text = {
    en: {
      download: os => `Download for ${os}`,
      preview: ' (preview)',
      latest: (link, kind, date) => `Latest version: ${link}${kind}, released ${date}.`,
      meta: (tag, kind) => `${tag}${kind} · Free and open source under the MIT license.`,
    },
    ru: {
      download: os => `Скачать для ${os}`,
      preview: ' (предварительная)',
      latest: (link, kind, date) => `Последняя версия: ${link}${kind}, вышла ${date}`,
      meta: (tag, kind) => `${tag}${kind} · Бесплатно и с открытым кодом по лицензии MIT.`,
    },
    uk: {
      download: os => `Завантажити для ${os}`,
      preview: ' (попередня)',
      latest: (link, kind, date) => `Остання версія: ${link}${kind}, вийшла ${date}`,
      meta: (tag, kind) => `${tag}${kind} · Безкоштовно та з відкритим кодом за ліцензією MIT.`,
    },
  }[lang];

  // Remember the language a visitor picks; the English page reads it to redirect.
  document.querySelectorAll('a[hreflang]').forEach(a =>
    a.addEventListener('click', () => { try { localStorage.setItem('lang', a.hreflang); } catch {} }));

  const ua = navigator.userAgent;
  const os = /Windows/i.test(ua) ? 'windows' : /Mac/i.test(ua) && !/iPhone|iPad/i.test(ua) ? 'mac' : /Linux/i.test(ua) && !/Android/i.test(ua) ? 'linux' : null;
  const names = { windows: 'Windows', mac: 'macOS', linux: 'Linux' };
  const primary = { windows: '-setup\\.exe$', mac: '_aarch64\\.dmg$', linux: '\\.AppImage$' };

  if (os) {
    const card = document.querySelector(`.os[data-os="${os}"]`);
    card.classList.add('current');
    card.querySelector('em').hidden = false;
    card.parentNode.prepend(card);
    document.getElementById('hero-dl-label').textContent = text.download(names[os]);
  }

  // The newest published release, prereleases included.
  fetch('https://api.github.com/repos/vkardin/snapt/releases?per_page=5')
    .then(r => r.ok ? r.json() : Promise.reject(r.status))
    .then(list => {
      const rel = list.find(r => !r.draft);
      if (!rel) return;
      const find = re => rel.assets.find(a => new RegExp(re).test(a.name));
      document.querySelectorAll('a[data-asset]').forEach(a => {
        const asset = find(a.dataset.asset);
        if (asset) a.href = asset.browser_download_url;
      });
      const date = new Date(rel.published_at).toLocaleDateString(lang, { year: 'numeric', month: 'long', day: 'numeric' });
      const kind = rel.prerelease ? text.preview : '';
      document.getElementById('release-line').innerHTML =
        text.latest(`<a href="${rel.html_url}">${rel.tag_name}</a>`, kind, date);
      document.getElementById('hero-meta').textContent = text.meta(rel.tag_name, kind);
      const main = os && find(primary[os]);
      if (main) document.getElementById('hero-dl').href = main.browser_download_url;
    })
    .catch(() => {});
})();
