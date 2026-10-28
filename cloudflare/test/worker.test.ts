import { test } from "node:test";
import assert from "node:assert/strict";
import worker, { publicBase, type Env } from "../src/worker.ts";

const token = "test-only-token-0123456789-abcdefghijk";
const png = Uint8Array.from(Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j4X8AAAAASUVORK5CYII=", "base64"));
function setup(base?: string) {
  const objects = new Map<string, Uint8Array>();
  let puts = 0;
  const env: Env = {
    ICON_UPLOAD_TOKEN: token,
    PUBLIC_IMAGE_BASE: base,
    ICONS: {
      async head(key) { return objects.has(key) ? {} : null; },
      async put(key, bytes, options) { assert.equal(options.httpMetadata.contentType, "image/png"); puts++; objects.set(key, bytes.slice()); return {}; },
      async get(key) { const bytes = objects.get(key); return bytes ? { size: bytes.length, body: new ReadableStream({ start(c) { c.enqueue(bytes); c.close(); } }) } : null; },
    },
  };
  return { env, objects, puts: () => puts };
}
function upload(body: BodyInit = png, headers: Record<string, string> = {}) {
  return new Request("https://icons.example.com/v1/icons", { method: "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "image/png", ...headers }, body });
}
test("requires token and configured authentication before touching storage", async () => {
  const { env, puts } = setup();
  assert.equal((await worker.fetch(upload(png, { Authorization: "Bearer wrong" }), env)).status, 401);
  assert.equal((await worker.fetch(upload(), { ...env, ICON_UPLOAD_TOKEN: "" })).status, 503);
  assert.equal(puts(), 0);
});
test("PNG type, signature, dimensions and truncated chunks are rejected", async () => {
  const { env, puts } = setup();
  assert.equal((await worker.fetch(upload(png, { "Content-Type": "text/plain" }), env)).status, 415);
  assert.equal((await worker.fetch(upload(new Uint8Array(64)), env)).status, 422);
  const hugeDimensions = png.slice(); new DataView(hugeDimensions.buffer).setUint32(16, 257);
  assert.equal((await worker.fetch(upload(hugeDimensions), env)).status, 422);
  assert.equal((await worker.fetch(upload(png.slice(0, -1)), env)).status, 422);
  assert.equal(puts(), 0);
});
test("streaming cap works without Content-Length and with an understated length", async () => {
  const { env, puts } = setup();
  for (const declared of [undefined, "1"]) {
    let cancelled = false, reads = 0;
    const stream = new ReadableStream<Uint8Array>({ pull(c) { reads++; c.enqueue(new Uint8Array(70 * 1024)); }, cancel() { cancelled = true; } });
    const headers: Record<string, string> = { Authorization: `Bearer ${token}`, "Content-Type": "image/png" };
    if (declared) headers["Content-Length"] = declared;
    const request = new Request("https://icons.example.com/v1/icons", { method: "POST", headers, body: stream, duplex: "half" } as RequestInit);
    assert.equal((await worker.fetch(request, env)).status, 413);
    assert.ok(cancelled); assert.ok(reads <= 3);
  }
  assert.equal(puts(), 0);
});
test("server hashes bytes, deduplicates, and GET/HEAD serve immutable PNG", async () => {
  const { env, puts } = setup("https://cdn.example.com");
  const first = await worker.fetch(upload(), env);
  assert.equal(first.status, 200);
  const result = await first.json() as { sha256: string; url: string };
  const expected = Buffer.from(await crypto.subtle.digest("SHA-256", png)).toString("hex");
  assert.deepEqual(result, { sha256: expected, url: `https://cdn.example.com/icons/${expected}.png` });
  await worker.fetch(upload(), env); assert.equal(puts(), 1);
  const response = await worker.fetch(new Request(`https://icons.example.com/icons/${expected}.png`), env);
  assert.equal(response.status, 200); assert.equal(response.headers.get("Content-Type"), "image/png");
  assert.match(response.headers.get("Cache-Control")!, /immutable/);
  assert.deepEqual(new Uint8Array(await response.arrayBuffer()), png);
  const head = await worker.fetch(new Request(result.url, { method: "HEAD" }), env);
  assert.equal(head.status, 200); assert.equal((await head.arrayBuffer()).byteLength, 0);
  assert.equal((await worker.fetch(new Request(`https://icons.example.com/icons/${"0".repeat(64)}.png`), env)).status, 404);
});
test("public origin default works; unsafe or ambiguous configured bases fail closed", async () => {
  const { env } = setup();
  const result = await (await worker.fetch(upload(), env)).json() as { url: string };
  assert.match(result.url, /^https:\/\/icons\.example\.com\/icons\/[a-f0-9]{64}\.png$/);
  for (const base of ["http://public.example.com", "https://localhost", "https://127.0.0.1", "https://10.0.0.1", "https://[::1]", "https://printer.local", "https://user:secret@cdn.example.com", "https://cdn.example.com/private", "https://cdn.example.com/?token=secret", "not a URL"]) {
    assert.throws(() => publicBase(base));
    const invalid = setup(base);
    assert.equal((await worker.fetch(upload(), invalid.env)).status, 503); assert.equal(invalid.puts(), 0);
  }
});
test("storage failures never reveal provider details", async () => {
  const { env } = setup(); env.ICONS.head = async () => { throw new Error("private provider credential detail"); };
  const response = await worker.fetch(upload(), env);
  assert.equal(response.status, 503); assert.equal((await response.text()).includes("private"), false);
});
test("upload URL, metadata and headers cannot choose an object name or leak into storage", async () => {
  const { env, objects } = setup();
  const request = new Request("https://icons.example.com/v1/icons?filename=C%3A%5CUsers%5Cprivate%5CEditor.exe", {
    method: "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "image/png", "X-Application-Name": "Private editor", "Content-Disposition": 'attachment; filename="private.png"' }, body: png,
  });
  const response = await worker.fetch(request, env);
  assert.equal(response.status, 200);
  const result = await response.text();
  assert.equal(objects.size, 1);
  assert.match([...objects.keys()][0], /^icons\/[a-f0-9]{64}\.png$/);
  assert.equal(result.includes("private"), false);
  assert.equal(result.includes("Editor"), false);
  assert.deepEqual([...objects.values()][0], png);
});
test("upload tokens accept exactly printable ASCII 32–1024, and bound Authorization", async () => {
  for (const length of [32, 1024]) {
    const { env } = setup(); env.ICON_UPLOAD_TOKEN = "a".repeat(length);
    const response = await worker.fetch(upload(png, { Authorization: `Bearer ${env.ICON_UPLOAD_TOKEN}` }), env);
    assert.equal(response.status, 200);
  }
  for (const invalid of ["a".repeat(31), "a".repeat(1025), "a".repeat(31) + " ", "a".repeat(31) + "é", "a".repeat(31) + "\n", "a".repeat(31) + "\x7f"]) {
    const { env, puts } = setup(); env.ICON_UPLOAD_TOKEN = invalid;
    assert.equal((await worker.fetch(upload(), env)).status, 503); assert.equal(puts(), 0);
  }
  const { env, puts } = setup();
  assert.equal((await worker.fetch(upload(png, { Authorization: `Bearer ${"a".repeat(1025)}` }), env)).status, 401);
  assert.equal(puts(), 0);
});
test("trailing DNS root dots cannot bypass local hostname restrictions", () => {
  for (const value of ["https://localhost.", "https://LOCALHOST...", "https://printer.local.", "https://service.internal.", "https://private.lan.", "https://router.home.", "https://example.test.", "https://host.invalid."]) {
    assert.throws(() => publicBase(value));
  }
  assert.equal(publicBase("https://images.example.com."), "https://images.example.com.");
});
