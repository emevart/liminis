// Synthetic edge-insertion oracle controls only; no public deployment claim.
// Cloudflare serves no insertion to some networks, so the template is covered
// here rather than by a live run of the public gate.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("check_public_recording_browser.mjs", import.meta.url), "utf8");
const start = source.indexOf("const knownEdgeInsertion = {"), end = source.indexOf("const controls = [", start);
assert.ok(start >= 0 && end > start);
// Exercise the actual shipped matcher without running the gate's host lifecycle.
const { knownEdgeInsertion, beaconScriptUrl, edgeInsertionFacts } = vm.runInNewContext(
  source.slice(start, end) + "\n({ knownEdgeInsertion, beaconScriptUrl, edgeInsertionFacts })",
  { URL, Buffer },
);

// Synthetic version, SRI and site token: real values are not needed in the repository.
const version = "v" + "a1b2c3d4".repeat(5) + "f0e1d2c3";
const integrity = "sha512-" + "A".repeat(86) + "==";
const tag = ({ ver = version, sri = integrity, token = "0".repeat(32), beaconVersion = "2024.11.0", extra = "" } = {}) =>
  Buffer.from(`<script type="module" src="https://static.cloudflareinsights.com/beacon.min.js/${ver}" integrity="${sri}" data-cf-beacon='{"version":"${beaconVersion}","token":"${token}","r":1,"spa":2}'${extra} crossorigin="anonymous"></script>`);

test("the reviewed beacon tag matches and surrenders its rotated version and SRI", () => {
  const facts = edgeInsertionFacts(tag());
  assert.ok(facts);
  assert.equal(facts.beaconUrl, `https://static.cloudflareinsights.com/beacon.min.js/${version}`);
  assert.equal(facts.integrity, integrity);
  assert.equal(facts.bytes, tag().byteLength);
});

test("a rotated version of any accepted length and a different SRI still match", () => {
  for (const ver of ["v" + "0".repeat(40), "v" + "f".repeat(64), "v" + "ce701823".repeat(5) + "92088814"]) {
    const sri = "sha512-" + "b".repeat(86) + "==";
    const facts = edgeInsertionFacts(tag({ ver, sri }));
    assert.ok(facts, ver);
    assert.equal(facts.beaconUrl, `https://static.cloudflareinsights.com/beacon.min.js/${ver}`);
    assert.equal(facts.integrity, sri);
  }
  // The insertion length moves with the version; the gate derives it from the size delta.
  assert.notEqual(edgeInsertionFacts(tag({ ver: "v" + "0".repeat(40) })).bytes, edgeInsertionFacts(tag()).bytes);
});

test("the one LF that closes the inserted line is tolerated, any other padding is not", () => {
  const lf = Buffer.concat([tag(), Buffer.from("\n")]);
  const facts = edgeInsertionFacts(lf);
  assert.ok(facts);
  assert.equal(facts.bytes, lf.byteLength);
  assert.equal(facts.beaconUrl, `https://static.cloudflareinsights.com/beacon.min.js/${version}`);
  for (const tail of ["\r\n", "\n\n", "\n ", " \n", "\r", "\t", " "]) {
    assert.equal(edgeInsertionFacts(Buffer.concat([tag(), Buffer.from(tail)])), null, JSON.stringify(tail));
  }
});

test("anything other than that one tag is refused, including a plausible near miss", () => {
  const refused = [
    tag({ ver: "v" + "0".repeat(39) }),
    tag({ ver: "v" + "0".repeat(65) }),
    tag({ ver: "v" + "A".repeat(40) }),
    tag({ ver: "v" + "0".repeat(40) + "?x=1" }),
    tag({ sri: "sha384-" + "A".repeat(86) + "==" }),
    tag({ sri: integrity.slice(0, -1) }),
    tag({ token: "0".repeat(31) }),
    tag({ token: "Z".repeat(32) }),
    tag({ beaconVersion: "2024.11" }),
    tag({ extra: " async" }),
    tag({ extra: ' onload="x()"' }),
    Buffer.concat([tag(), tag()]),
    Buffer.concat([tag(), Buffer.from("<!-- -->")]),
    Buffer.concat([Buffer.from("\n"), tag()]),
    Buffer.concat([Buffer.from("\n"), tag(), Buffer.from("\n")]),
    Buffer.concat([tag(), Buffer.from("\n"), tag(), Buffer.from("\n")]),
    Buffer.from(tag().toString("utf8").replace("static.cloudflareinsights.com", "static.cloudflareinsights.com.example")),
    Buffer.from(tag().toString("utf8").replace("beacon.min.js", "beacon.js")),
    Buffer.from(tag().toString("utf8").replace('type="module"', 'type="text/javascript"')),
    Buffer.from(tag().toString("utf8").replace('crossorigin="anonymous"', "")),
    Buffer.from("<script src=\"/local.js\"></script>"),
    Buffer.alloc(0),
  ];
  for (const bytes of refused) assert.equal(edgeInsertionFacts(bytes), null, bytes.toString("utf8").slice(0, 60));
});

test("only an exact beacon script URL is an allowed non-public request", () => {
  const accepted = `https://static.cloudflareinsights.com/beacon.min.js/${version}`;
  assert.equal(beaconScriptUrl(accepted), accepted);
  for (const value of [
    `http://static.cloudflareinsights.com/beacon.min.js/${version}`,
    `https://static.cloudflareinsights.com/beacon.min.js/${version}?r=1`,
    `https://static.cloudflareinsights.com/beacon.min.js/${version}#x`,
    `https://static.cloudflareinsights.com/beacon.min.js/${version}/extra`,
    `https://static.cloudflareinsights.com/other/${version}`,
    `https://cloudflareinsights.com/beacon.min.js/${version}`,
    "https://static.cloudflareinsights.com/beacon.min.js/",
    "https://liminis.dev/observe.html",
    "not a url",
    "",
  ]) assert.equal(beaconScriptUrl(value), null, value);
});

test("the source byte pin of the unmodified document is retained", () => {
  assert.equal(knownEdgeInsertion.sourceBytes, 6954);
  assert.equal(knownEdgeInsertion.sourceSha256, "105c5a860651769c0310c4185a96d7ad2442c3726b51779a8afa1f1707d0b72e");
  assert.equal(knownEdgeInsertion.offset, 6938);
  assert.ok(knownEdgeInsertion.maxBytes > 0 && knownEdgeInsertion.maxBytes <= 1024);
});
