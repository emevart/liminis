import assert from "node:assert/strict";
import test from "node:test";
import { playbackIndex, validFrameRate } from "./playback.mjs";

test("slow playback holds each recorded frame for its selected duration", () => {
  assert.equal(playbackIndex(0, 9999, 0.1, 201), 0);
  assert.equal(playbackIndex(0, 10000, 0.1, 201), 1);
  assert.equal(playbackIndex(0, 19999, 0.1, 201), 1);
});
test("playback advances from the paused frame without synthetic samples", () => {
  assert.equal(playbackIndex(70, 2999, 1, 201), 72);
  assert.equal(playbackIndex(70, 3000, 1, 201), 73);
});
test("late rendering clamps at the final recorded frame", () => {
  assert.equal(playbackIndex(199, 60000, 60, 201), 200);
});
test("one-frame and negative clock deltas do not move the cursor", () => {
  assert.equal(playbackIndex(0, 60000, 1, 1), 0);
  assert.equal(playbackIndex(20, -100, 1, 201), 20);
});
test("manual frame rate rejects invalid and unbounded inputs", () => {
  for (const value of [0, -1, "", "wrong", NaN, Infinity, 0.01, 60.1]) assert.equal(validFrameRate(value), false);
  for (const value of [0.05, 0.1, "0.25", 1, 60]) assert.equal(validFrameRate(value), true);
});
