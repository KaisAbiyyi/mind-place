/**
 * Captured pictures come back from the native `get_asset` command as data URLs.
 * These rules are the same as the classic reader's (`src/reader.ts`): only raster
 * image types, a size cap, an exact `data:<mime>;base64,` prefix and a strict
 * base64 body. The classic code is intentionally not touched; a test pins both
 * to the same behavior.
 */
export const ALLOWED_IMAGE_MIME_TYPES: ReadonlySet<string> = new Set([
  "image/avif",
  "image/gif",
  "image/jpeg",
  "image/png",
  "image/webp",
]);

const MAX_DATA_URL_CHARS = 32 * 1024 * 1024;

export function isSafeImageDataUrl(dataUrl: string, mimeType: string): boolean {
  if (dataUrl.length > MAX_DATA_URL_CHARS) return false;
  const prefix = `data:${mimeType};base64,`;
  if (!dataUrl.startsWith(prefix)) return false;
  return /^[A-Za-z0-9+/]*={0,2}$/.test(dataUrl.slice(prefix.length));
}
