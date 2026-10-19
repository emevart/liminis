// Synthetic PNG oracle controls only; no model states or browser PASS.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";
import { deflateSync, inflateSync } from "node:zlib";

const source = readFileSync(new URL("check_physical_3d_browser.mjs", import.meta.url), "utf8");
const start = source.indexOf("function pngPixels(bytes) {"), end = source.indexOf("function contrast(", start);
assert.ok(start >= 0 && end > start);
// Exercise the actual bounded QA decoder/comparator without executing its
// browser/host lifecycle. No alternate implementation supplies expected pixels.
const { pngPixels, pixelDifference, silhouetteJitter } = vm.runInNewContext(source.slice(start, end) + "\n({ pngPixels, pixelDifference, silhouetteJitter })", { Buffer, assert, inflateSync });
const rgba = Buffer.from([0, 17, 255, 255, 123, 44, 3, 255, 9, 10, 11, 255, 250, 128, 64, 255]);
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) { crc ^= byte; for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0); }
  return (crc ^ 0xffffffff) >>> 0;
}
function chunk(type, bytes) {
  const name = Buffer.from(type), result = Buffer.alloc(bytes.length + 12);
  result.writeUInt32BE(bytes.length); name.copy(result, 4); bytes.copy(result, 8);
  result.writeUInt32BE(crc32(Buffer.concat([name, bytes])), bytes.length + 8); return result;
}
function png({ width = 2, height = 2, channels = 4, filter = 0, level = 9, pixels = rgba } = {}) {
  const plain = Buffer.alloc(width * height * channels);
  for (let i = 0; i < width * height; i++) pixels.copy(plain, i * channels, i * 4, i * 4 + channels);
  const stride = width * channels, raw = Buffer.alloc(height * (stride + 1));
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = filter;
    for (let x = 0; x < stride; x++) {
      const i = y * stride + x, left = x >= channels ? plain[i - channels] : 0, above = y ? plain[i - stride] : 0, corner = y && x >= channels ? plain[i - stride - channels] : 0;
      let predictor = [0, left, above, Math.floor((left + above) / 2)][filter];
      if (filter === 4) {
        const target = left + above - corner, candidates = [left, above, corner];
        predictor = candidates.reduce((best, value) => Math.abs(target - value) < Math.abs(target - best) ? value : best);
      }
      raw[y * (stride + 1) + x + 1] = (plain[i] - predictor + 256) % 256;
    }
  }
  const header = Buffer.alloc(13); header.writeUInt32BE(width); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = channels === 4 ? 6 : 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", header), chunk("IDAT", deflateSync(raw, { level })), chunk("IEND", Buffer.alloc(0))]);
}
test("all five PNG filters and opaque RGB decode to exact known RGBA regardless of encoding", () => {
  const baselineBytes = png({ level: 0 }), baseline = pngPixels(baselineBytes);
  assert.deepEqual(baseline.rgba, rgba);
  for (const channels of [3, 4]) for (const filter of [0, 1, 2, 3, 4]) {
    const bytes = png({ channels, filter }); assert.notDeepEqual(bytes, baselineBytes);
    const image = pngPixels(bytes); assert.deepEqual(image.rgba, rgba);
    const diff = pixelDifference(baseline, image); assert.equal(diff.equal, true); assert.equal(diff.changedPixels, 0); assert.equal(diff.bounds, null);
    assert.deepEqual(Array.from(image.at(1, 1)), [250, 128, 64]);
  }
});
test("one RGB or alpha channel byte changes one pixel and fails exact identity", () => {
  const baseline = pngPixels(png());
  for (const channel of [0, 1, 2, 3]) {
    const changed = Buffer.from(rgba); changed[12 + channel] ^= 1;
    const diff = pixelDifference(baseline, pngPixels(png({ pixels: changed })));
    assert.equal(diff.equal, false); assert.equal(diff.sameDimensions, true); assert.equal(diff.changedPixels, 1);
    assert.deepEqual(JSON.parse(JSON.stringify(diff.bounds)), { left: 1, top: 1, right: 1, bottom: 1 });
  }
});
test("equal RGBA bytes at different dimensions fail; multiple pixel bounds include both changes", () => {
  const baseline = pngPixels(png()), reshaped = pngPixels(png({ width: 4, height: 1 }));
  assert.deepEqual(baseline.rgba, reshaped.rgba);
  assert.equal(pixelDifference(baseline, reshaped).equal, false); assert.equal(pixelDifference(baseline, reshaped).sameDimensions, false);
  const changed = Buffer.from(rgba); changed[0]++; changed[15]--;
  const diff = pixelDifference(baseline, pngPixels(png({ pixels: changed })));
  assert.equal(diff.changedPixels, 2); assert.deepEqual(JSON.parse(JSON.stringify(diff.bounds)), { left: 0, top: 0, right: 1, bottom: 1 });
});
// The chamber background and the measured window of failing CI run 39130380525:
// a column of antialiased edge beside the background. The values were decoded
// from its attached PNGs; these colours are oracle data only, not model state.
const background = [8, 13, 11];
const measuredWindow = [
  [background, background, background, background, background],
  [background, background, [21, 30, 26], [35, 47, 40], [41, 54, 47]],
  [background, [21, 30, 26], [29, 40, 34], [12, 18, 16], background],
  [background, [35, 47, 40], [12, 18, 16], background, background],
  [background, [41, 54, 47], background, background, background],
];
function field(rows) {
  const height = rows.length, width = rows[0].length, pixels = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) { const i = (y * width + x) * 4; pixels[i] = rows[y][x][0]; pixels[i + 1] = rows[y][x][1]; pixels[i + 2] = rows[y][x][2]; pixels[i + 3] = rows[y][x][3] ?? 255; }
  return { width, height, pixels };
}
const decoded = (rows) => { const { width, height, pixels } = field(rows); return pngPixels(png({ width, height, pixels })); };
const clone = (rows) => rows.map((row) => row.map((pixel) => [...pixel]));

test("a measured silhouette pixel may jitter one LSB; anything farther is rejected", () => {
  const baseline = decoded(measuredWindow);
  const jittered = clone(measuredWindow); jittered[2][2][0] += 1;
  const held = pixelDifference(baseline, decoded(jittered));
  assert.deepEqual([held.equal, held.held, held.changedPixels, held.tolerated, held.rejected], [false, true, 1, 1, 0]);
  for (const mutate of [
    (rows) => { rows[2][2][0] += silhouetteJitter.maxChannelDelta + 1; },
    (rows) => { rows[2][2][1] -= silhouetteJitter.maxChannelDelta + 1; },
    (rows) => { rows[2][2][3] = 254; },
    (rows) => { rows[4][1][0] += 1; },
  ]) {
    const rows = clone(measuredWindow); mutate(rows);
    const diff = pixelDifference(baseline, decoded(rows));
    assert.equal(diff.held, false); assert.equal(diff.rejected, 1); assert.equal(diff.tolerated, 0); assert.equal(diff.rejectedSamples.length, 1);
  }
});

test("one LSB away from the camera background is rejected, however smooth the surroundings", () => {
  const rows = Array.from({ length: 5 }, (_, y) => Array.from({ length: 5 }, (_, x) => [28 + x, 40 + y, 34]));
  const baseline = decoded(rows);
  const changed = clone(rows); changed[2][2][0] += 1;
  const diff = pixelDifference(baseline, decoded(changed));
  assert.equal(diff.held, false); assert.equal(diff.rejected, 1);
  // The same pixel with the same shift is held once the background borders it.
  const bordered = clone(rows); bordered[1][2] = [...background];
  const borderedBaseline = decoded(bordered);
  const borderedChanged = clone(bordered); borderedChanged[2][2][0] += 1;
  const allowed = pixelDifference(borderedBaseline, decoded(borderedChanged));
  assert.equal(allowed.held, true); assert.equal(allowed.tolerated, 1);
});

test("the count of tolerated silhouette pixels is capped", () => {
  const strip = () => [
    Array.from({ length: 80 }, () => [...background]),
    Array.from({ length: 80 }, (_, x) => [20 + x, 40, 34]),
    Array.from({ length: 80 }, () => [...background]),
  ];
  const baseline = decoded(strip());
  for (const [count, expected] of [[silhouetteJitter.maxPixels, true], [silhouetteJitter.maxPixels + 1, false]]) {
    const rows = strip(); for (let x = 1; x <= count; x++) rows[1][x][0] += 1;
    const diff = pixelDifference(baseline, decoded(rows));
    assert.equal(diff.tolerated, count); assert.equal(diff.rejected, 0); assert.equal(diff.held, expected, `tolerated ${count}`);
  }
});

test("invalid signature, filter and oversized dimensions are rejected", () => {
  assert.throws(() => pngPixels(Buffer.alloc(20)));
  assert.throws(() => pngPixels(png({ filter: 5 })));
  const largeHeader = Buffer.alloc(13); largeHeader.writeUInt32BE(4096); largeHeader.writeUInt32BE(4096, 4); largeHeader[8] = 8; largeHeader[9] = 6;
  const oversized = Buffer.concat([png().subarray(0, 8), chunk("IHDR", largeHeader), chunk("IEND", Buffer.alloc(0))]);
  assert.throws(() => pngPixels(oversized));
});
