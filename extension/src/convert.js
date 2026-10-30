// Converting images to PNG or JPG, shared by the background script and the
// offscreen document. Everything happens locally; nothing leaves the browser.

/* exported convertImage, decodeInDocument, blobToDataUrl */

const JPEG_QUALITY = 0.92;
// The size an SVG without its own width and height is drawn at.
const SVG_FALLBACK_SIZE = 1024;

// Converts `blob` to `type` ("image/png" or "image/jpeg"). `fallback` decodes
// formats that only a document can, such as SVG, and gets the blob as a data URL.
async function convertImage(blob, type, fallback) {
  let bitmap;
  try {
    bitmap = await createImageBitmap(blob);
  } catch {
    return fallback(await blobToDataUrl(blob), type);
  }
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  draw(canvas.getContext('2d'), bitmap, bitmap.width, bitmap.height, type);
  bitmap.close();
  return canvas.convertToBlob({ type, quality: JPEG_QUALITY });
}

// Decodes a data URL with an <img> and returns the converted image as a data URL.
async function decodeInDocument(dataUrl, type) {
  const img = new Image();
  img.src = dataUrl;
  await img.decode();
  const width = img.naturalWidth || SVG_FALLBACK_SIZE;
  const height = img.naturalHeight || SVG_FALLBACK_SIZE;
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  draw(canvas.getContext('2d'), img, width, height, type);
  return canvas.toDataURL(type, JPEG_QUALITY);
}

// JPEG has no transparency, so transparent pixels become white rather than black.
function draw(ctx, source, width, height, type) {
  if (type === 'image/jpeg') {
    ctx.fillStyle = '#fff';
    ctx.fillRect(0, 0, width, height);
  }
  ctx.drawImage(source, 0, 0, width, height);
}

function blobToDataUrl(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}
