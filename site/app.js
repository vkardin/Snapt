(() => {
  const lang = document.documentElement.lang;
  const text = {
    en: {
      download: os => `Download for ${os}`,
      preview: ' (preview)',
      latest: (link, kind, date) => `Latest version: ${link}${kind}, released ${date}.`,
      meta: (tag, kind) => `${tag}${kind} · Free and open source under the MIT license.`,
      copy: 'Copy address',
      copied: 'Copied',
    },
    ru: {
      download: os => `Скачать для ${os}`,
      preview: ' (предварительная)',
      latest: (link, kind, date) => `Последняя версия: ${link}${kind}, вышла ${date}`,
      meta: (tag, kind) => `${tag}${kind} · Бесплатно и с открытым кодом по лицензии MIT.`,
      copy: 'Скопировать адрес',
      copied: 'Скопировано',
    },
    uk: {
      download: os => `Завантажити для ${os}`,
      preview: ' (попередня)',
      latest: (link, kind, date) => `Остання версія: ${link}${kind}, вийшла ${date}`,
      meta: (tag, kind) => `${tag}${kind} · Безкоштовно та з відкритим кодом за ліцензією MIT.`,
      copy: 'Скопіювати адресу',
      copied: 'Скопійовано',
    },
  }[lang];

  // Remember the language a visitor picks; the English page reads it to redirect.
  document.querySelectorAll('a[hreflang]').forEach(a =>
    a.addEventListener('click', () => { try { localStorage.setItem('lang', a.hreflang); } catch {} }));

  // Donation wallets. The QR codes in qr/<id>.svg encode the same addresses.
  const wallets = [
    { id: 'usdt-trc20', name: 'USDT', net: 'TRON · TRC-20', color: '#26a17b', sign: '₮', addr: 'TWW4iFFTPtvDBu3NH2iy6WFKQZgkq3TRhN' },
    { id: 'btc', name: 'Bitcoin', net: 'BTC', color: '#f7931a', sign: '₿', addr: 'bc1qk2eu8d53x08p0m6k3z5ernv55v2clr4ywarp7w' },
    { id: 'eth', name: 'Ethereum', net: 'ETH · ERC-20', color: '#627eea', sign: 'Ξ', addr: '0x3D0765a406dD7AEDF9614bB3742A9084dfF42f18' },
    { id: 'sol', name: 'Solana', net: 'SOL', color: '#9945ff', sign: 'S', addr: 'G1iMvFiS6yzTouDnZcaFHimkdicNmTtuxhYjZW1UTpPE' },
  ];
  const box = document.getElementById('wallets');
  for (const w of wallets) {
    const card = document.createElement('div');
    card.className = 'card wallet';
    card.innerHTML = `<div class="coin"><i></i><div><b></b><small></small></div></div>
      <div class="qr"><img width="300" height="300" alt=""></div><div class="addr"></div>
      <button type="button" class="copy"></button>`;
    const sign = card.querySelector('.coin i');
    sign.textContent = w.sign;
    sign.style.background = w.color;
    card.querySelector('.coin b').textContent = w.name;
    card.querySelector('.coin small').textContent = w.net;
    const img = card.querySelector('.qr img');
    img.src = box.dataset.qr + w.id + '.svg';
    img.alt = `QR ${w.name}`;
    const addr = card.querySelector('.addr');
    addr.textContent = w.addr;
    const btn = card.querySelector('.copy');
    btn.textContent = text.copy;
    btn.addEventListener('click', async () => {
      try { await navigator.clipboard.writeText(w.addr); }
      catch {
        const range = document.createRange();
        range.selectNodeContents(addr);
        const sel = getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
        document.execCommand('copy');
      }
      btn.classList.add('done');
      btn.textContent = text.copied;
      setTimeout(() => { btn.classList.remove('done'); btn.textContent = text.copy; }, 1800);
    });
    box.appendChild(card);
  }

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
