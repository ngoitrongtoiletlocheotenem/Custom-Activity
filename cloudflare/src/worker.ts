interface R2Object { body: ReadableStream<Uint8Array> | null; size: number }
interface IconBucket {
  head(key: string): Promise<unknown | null>;
  get(key: string): Promise<R2Object | null>;
  put(key: string, value: Uint8Array, options: { httpMetadata: { contentType: string; cacheControl: string } }): Promise<unknown>;
}
export interface Env { ICONS: IconBucket; ICON_UPLOAD_TOKEN: string; PUBLIC_IMAGE_BASE?: string }

const LIMIT = 128 * 1024;
const IMMUTABLE = "public, max-age=31536000, immutable";
const PNG_SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10];
class InputError extends Error { status: number; constructor(status: number, message: string) { super(message); this.status = status; } }
function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), { status, headers: { "Content-Type": "application/json", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" } });
}
export function publicBase(value: string): string {
  let url: URL;
  try { url = new URL(value); } catch { throw new InputError(503, "Public image base is not configured correctly."); }
  const host = url.hostname.toLowerCase().replace(/\.+$/, "");
  // An HTTPS DNS origin is required. IP literals (including private/loopback),
  // local/reserved suffixes and embedded credentials are never public icon URLs.
  if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash ||
      (url.pathname !== "/" && url.pathname !== "") || (url.port && url.port !== "443") ||
      !host.includes(".") || host.startsWith("[") || /^[\d.]+$/.test(host) ||
      ["localhost", ".localhost", ".local", ".internal", ".lan", ".home", ".test", ".invalid"].some(s => host === s || host.endsWith(s))) {
    throw new InputError(503, "Public image base must be a public HTTPS DNS origin.");
  }
  if (url.origin.length + "/icons/".length + 64 + ".png".length > 300) throw new InputError(503, "Public image URL would exceed Discord's 300-character limit.");
  return url.origin;
}
async function digest(value: Uint8Array): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", new Uint8Array(value).buffer));
}
async function authorized(request: Request, expected: string): Promise<boolean> {
  const header = request.headers.get("Authorization") ?? "";
  if (!header.startsWith("Bearer ") || header.length > 1031) return false;
  const encoder = new TextEncoder();
  const [a, b] = await Promise.all([digest(encoder.encode(header.slice(7))), digest(encoder.encode(expected))]);
  let difference = 0;
  for (let i = 0; i < a.length; i++) difference |= a[i] ^ b[i];
  return difference === 0;
}
async function boundedBody(request: Request): Promise<Uint8Array> {
  const declared = request.headers.get("Content-Length");
  if (declared && (!/^\d+$/.test(declared) || Number(declared) > LIMIT)) throw new InputError(413, "PNG exceeds 128 KiB.");
  if (!request.body) throw new InputError(422, "PNG body is required.");
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) break;
      total += next.value.byteLength;
      if (total > LIMIT) { await reader.cancel(); throw new InputError(413, "PNG exceeds 128 KiB."); }
      chunks.push(next.value);
    }
  } finally { reader.releaseLock(); }
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return bytes;
}
export function validatePng(bytes: Uint8Array): void {
  if (bytes.length < 45 || !PNG_SIGNATURE.every((b, i) => bytes[i] === b)) throw new InputError(422, "A PNG image is required.");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const ascii = (offset: number) => String.fromCharCode(...bytes.subarray(offset, offset + 4));
  if (view.getUint32(8) !== 13 || ascii(12) !== "IHDR") throw new InputError(422, "PNG IHDR is invalid.");
  const width = view.getUint32(16), height = view.getUint32(20);
  if (width < 1 || height < 1 || width > 256 || height > 256) throw new InputError(422, "PNG dimensions must be 1–256 pixels.");
  const depth = bytes[24], color = bytes[25];
  const depths: Record<number, number[]> = { 0: [1, 2, 4, 8, 16], 2: [8, 16], 3: [1, 2, 4, 8], 4: [8, 16], 6: [8, 16] };
  if (!depths[color]?.includes(depth) || bytes[26] !== 0 || bytes[27] !== 0 || bytes[28] > 1) throw new InputError(422, "PNG encoding is invalid.");
  let offset = 8, imageData = false, ended = false;
  while (offset + 12 <= bytes.length) {
    const length = view.getUint32(offset);
    if (length > bytes.length - offset - 12) throw new InputError(422, "PNG chunk is truncated.");
    const kind = ascii(offset + 4);
    if (kind === "IDAT") imageData = true;
    offset += length + 12;
    if (kind === "IEND") { if (length !== 0 || offset !== bytes.length) throw new InputError(422, "PNG end is invalid."); ended = true; break; }
  }
  if (!imageData || !ended) throw new InputError(422, "PNG image data is incomplete.");
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    try {
      const url = new URL(request.url);
      const image = /^\/icons\/([a-f0-9]{64})\.png$/.exec(url.pathname);
      if (image && (request.method === "GET" || request.method === "HEAD")) {
        const object = await env.ICONS.get(`icons/${image[1]}.png`);
        if (!object) return json({ error: "Image not found." }, 404);
        return new Response(request.method === "HEAD" ? null : object.body, { headers: {
          "Content-Type": "image/png", "Content-Length": String(object.size), "Cache-Control": IMMUTABLE,
          "ETag": `"${image[1]}"`, "X-Content-Type-Options": "nosniff", "Content-Security-Policy": "default-src 'none'; sandbox",
        } });
      }
      if (url.pathname !== "/v1/icons") return json({ error: "Not found." }, 404);
      if (request.method !== "POST") return new Response(null, { status: 405, headers: { Allow: "POST" } });
      if (!env.ICON_UPLOAD_TOKEN || !/^[\x21-\x7e]{32,1024}$/.test(env.ICON_UPLOAD_TOKEN)) return json({ error: "Upload authentication is not configured." }, 503);
      if (!await authorized(request, env.ICON_UPLOAD_TOKEN)) return json({ error: "Unauthorized." }, 401);
      if (request.headers.get("Content-Type")?.split(";", 1)[0].trim().toLowerCase() !== "image/png") throw new InputError(415, "Content-Type must be image/png.");
      const base = publicBase(env.PUBLIC_IMAGE_BASE || url.origin);
      const bytes = await boundedBody(request);
      validatePng(bytes);
      const hash = [...await digest(bytes)].map(b => b.toString(16).padStart(2, "0")).join("");
      const key = `icons/${hash}.png`;
      if (!await env.ICONS.head(key)) await env.ICONS.put(key, bytes, { httpMetadata: { contentType: "image/png", cacheControl: IMMUTABLE } });
      return json({ url: `${base}/${key}`, sha256: hash });
    } catch (error) {
      if (error instanceof InputError) return json({ error: error.message }, error.status);
      // Never log request headers, bodies, credentials, object names or provider errors.
      return json({ error: "Icon storage is temporarily unavailable." }, 503);
    }
  },
};
