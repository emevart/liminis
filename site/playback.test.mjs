import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { PlaybackClock, defaultPlaybackRate, sampleIndexAt, validPlaybackRate } from "./playback.mjs";

const irregularTimes = [0, 10, 20, 23];

test("physical 1× advances model seconds and holds only the last observed state", () => {
  const clock = new PlaybackClock(irregularTimes, 1);
  clock.start(100);
  assert.equal(clock.advance(10_099).playhead, 9.999);
  assert.equal(clock.advance(10_099).index, 0);
  assert.equal(clock.advance(10_100).index, 1);
  const between = clock.advance(23_099);
  assert.equal(between.playhead, 22.999);
  assert.equal(between.stateTime, 20);
  assert.equal(between.index, 2);
  assert.equal(between.ended, false);
  const final = clock.advance(23_100);
  assert.equal(final.playhead, 23);
  assert.equal(final.index, 3);
  assert.equal(final.playing, false);
  assert.equal(final.ended, true);
});

test("floor selection handles unequal sample gaps, exact boundaries, and late input", () => {
  const times = [2, 2.25, 14, 14.01, 100];
  for (const [time, expected] of [[0, 0], [2, 0], [2.2499, 0], [2.25, 1], [13.99, 1], [14, 2], [14.005, 2], [14.01, 3], [99, 3], [100, 4], [1e9, 4]]) {
    assert.equal(sampleIndexAt(times, time), expected);
  }
});

test("fractional playhead survives pause, idle time, and resume", () => {
  const clock = new PlaybackClock(irregularTimes, 2);
  clock.start(500);
  const paused = clock.pause(1625);
  assert.equal(paused.playhead, 2.25);
  assert.equal(paused.playing, false);
  assert.equal(clock.advance(100_000).playhead, 2.25);
  clock.start(100_000);
  assert.equal(clock.advance(100_250).playhead, 2.75);
});

test("rate changes commit old elapsed time before reanchoring without quantization", () => {
  const clock = new PlaybackClock([0, 10, 100], 1);
  clock.start(1000);
  const changed = clock.setRate("4", 2250);
  assert.equal(changed.playhead, 1.25);
  assert.equal(changed.rate, 4);
  assert.equal(changed.playing, true);
  assert.equal(changed.durationSeconds, 25);
  assert.equal(clock.advance(2750).playhead, 3.25);
  clock.pause(2750);
  clock.setRate(8, 9000);
  assert.equal(clock.advance(10_000).playhead, 3.25);
  assert.equal(clock.playing, false);
});

test("seek uses model time, pauses, floors the state, and clamps boundaries", () => {
  const clock = new PlaybackClock([5, 10, 23], 1);
  clock.start(0);
  const sought = clock.seek(12.5, 1000);
  assert.equal(sought.playhead, 12.5);
  assert.equal(sought.index, 1);
  assert.equal(sought.stateTime, 10);
  assert.equal(sought.playing, false);
  assert.equal(clock.advance(8000).playhead, 12.5);
  assert.equal(clock.seek(-200, 8000).playhead, 5);
  assert.equal(clock.seek(200, 8000).playhead, 23);
  assert.equal(clock.seek(200, 8000).ended, true);
});

test("previous and next pause at exact real samples with bounded endpoints", () => {
  const clock = new PlaybackClock(irregularTimes, 1);
  clock.seek(15.5, 0);
  assert.equal(clock.step(-1, 0).playhead, 0);
  assert.equal(clock.step(-1, 0).playhead, 0);
  assert.equal(clock.step(1, 0).playhead, 10);
  clock.start(0);
  const next = clock.step(1, 2500);
  assert.equal(next.playhead, 20);
  assert.equal(next.playing, false);
  assert.equal(clock.step(1, 2500).playhead, 23);
  assert.equal(clock.step(1, 2500).playhead, 23);
});

test("end is stable until explicit replay, which starts the same recording", () => {
  const clock = new PlaybackClock(irregularTimes, 2);
  clock.start(0);
  assert.equal(clock.advance(11_500).ended, true);
  assert.equal(clock.advance(200_000).playhead, 23);
  const replay = clock.start(200_000);
  assert.equal(replay.playhead, 0);
  assert.equal(replay.index, 0);
  assert.equal(replay.playing, true);
  assert.equal(clock.advance(201_000).playhead, 2);
});

test("visibility pause excludes all hidden time and has no automatic resume or backlog", () => {
  const clock = new PlaybackClock([0, 10, 1000], 1);
  clock.start(0);
  clock.pause(1250); // The visibility handler pauses and cancels RAF here.
  assert.equal(clock.advance(3_600_000).playhead, 1.25);
  assert.equal(clock.advance(3_600_000).playing, false);
  clock.start(3_600_000); // Explicit user resume establishes a new wall anchor.
  const resumed = clock.advance(3_600_016);
  assert.equal(resumed.playhead, 1.266);
  assert.equal(resumed.index, 0);
});

test("manual rate accepts all positive finite numbers but rejects coercion traps", () => {
  for (const value of [Number.MIN_VALUE, 0.01, 0.05, 1, "0.25", " 2.5 ", 60.1, 100_000_000, Number.MAX_VALUE]) {
    assert.equal(validPlaybackRate(value), true, String(value));
  }
  for (const value of [0, -1, "", " ", "wrong", "NaN", "Infinity", NaN, Infinity, -Infinity, null, undefined, true, false, [], [2], {}, 1n]) {
    assert.equal(validPlaybackRate(value), false, String(value));
  }
});

test("invalid rate changes never commit elapsed time or mutate the playing clock", () => {
  const clock = new PlaybackClock([0, 10, 100], 1);
  clock.start(0);
  const before = clock.advance(1250);
  for (const value of [0, null, false, "", NaN, Infinity]) assert.throws(() => clock.setRate(value, 9000), RangeError);
  assert.deepEqual(clock.advance(1250), before);
  assert.equal(clock.advance(2250).playhead, 2.25);
});

test("strict frame times and all clock inputs reject nonfinite or malformed values", () => {
  for (const times of [[], null, Array(1), [0, , 2], [0, NaN], [0, Infinity], [-1, 0], [0, 0], [1, 0], ["0", 1], [null]]) {
    assert.throws(() => new PlaybackClock(times), RangeError);
  }
  assert.throws(() => new PlaybackClock([0, 1], null), RangeError);
  const clock = new PlaybackClock([0, 10], 1);
  for (const value of [NaN, Infinity, null, "0"]) assert.throws(() => clock.start(value), RangeError);
  for (const value of [NaN, Infinity, null, false, ""]) assert.throws(() => clock.seek(value, 0), RangeError);
  for (const value of [NaN, Infinity, null, "1", 0.5]) assert.throws(() => clock.step(value, 0), RangeError);
});

test("single-frame recordings have no invented duration, sample, or advancing state", () => {
  const clock = new PlaybackClock([42]);
  assert.equal(clock.rate, 1);
  const state = clock.start(0);
  assert.equal(state.playhead, 42);
  assert.equal(state.durationSeconds, 0);
  assert.equal(state.ended, true);
  assert.equal(state.playing, false);
  assert.deepEqual(clock.advance(1e9), state);
  assert.equal(clock.step(1, 1e9).index, 0);
});

test("finite huge rates and wall deltas clamp safely without overflow or hidden cap", () => {
  const fast = new PlaybackClock([0, 1, 1e300], Number.MAX_VALUE);
  fast.start(0);
  assert.equal(fast.advance(Number.MAX_VALUE).playhead, 1e300);
  assert.equal(fast.advance(Number.MAX_VALUE).ended, true);
  const uncapped = new PlaybackClock([0, 10_000_000], 1_000_000);
  uncapped.start(0);
  assert.equal(uncapped.advance(1000).playhead, 1_000_000);
  const tiny = new PlaybackClock([0, 10], Number.MIN_VALUE);
  tiny.start(0);
  const slow = tiny.advance(1000);
  assert.equal(slow.playhead, Number.MIN_VALUE);
  assert.equal(slow.durationSeconds, Infinity);
  assert.equal(slow.ended, false);
  const short = new PlaybackClock([0, Number.MIN_VALUE], Number.MAX_VALUE);
  short.start(0);
  assert.equal(short.advance(0).playhead, 0);
  assert.equal(short.advance(1).ended, true);
});

test("negative elapsed wall deltas hold the model anchor", () => {
  const clock = new PlaybackClock(irregularTimes, 1);
  clock.seek(12.5, 1000);
  clock.start(1000);
  assert.equal(clock.advance(900).playhead, 12.5);
});

test("30 and 60 FPS schedules preserve the same continuous model duration", () => {
  const times = [0, 50, 100, 137.25];
  const rate = 4;
  for (const fps of [30, 60]) {
    const clock = new PlaybackClock(times, rate);
    clock.start(100);
    const durationMs = times.at(-1) / rate * 1000;
    for (let elapsed = 0; elapsed < durationMs; elapsed += 1000 / fps) clock.advance(100 + elapsed);
    const justBefore = clock.advance(100 + durationMs - 0.01);
    assert.equal(justBefore.ended, false);
    const end = clock.advance(100 + durationMs);
    assert.equal(end.ended, true);
    assert.equal(end.durationSeconds, 34.3125);
    assert.equal(end.index, 3);
  }
});

test("default overview and sample cadence use true horizons of unchanged public recordings", async () => {
  const cases = [
    ["cell-chamber-seed-42.json", 300_000, 2500, 1500],
    ["cell-chamber-seed-42-100k.json", 3_000_000, 25_000, 15_000],
    ["cell-chamber-seed-42-1m.json", 30_000_000, 250_000, 150_000],
  ];
  for (const [filename, horizon, rate, sampleSeconds] of cases) {
    const data = JSON.parse(await readFile(new URL(`./data/${filename}`, import.meta.url), "utf8"));
    const times = data.frames.map((frame) => frame.sim_time);
    const clock = new PlaybackClock(times);
    assert.equal(times.length, 201);
    assert.equal(clock.horizon, horizon);
    assert.equal(defaultPlaybackRate(times), rate);
    assert.equal(clock.rate, rate);
    assert.equal(times[1] - times[0], sampleSeconds);
    assert.equal(clock.advance(0).durationSeconds, 120);
    clock.start(0);
    const before = clock.advance(599);
    assert.equal(before.index, 0);
    assert.ok(before.playhead < sampleSeconds);
    assert.equal(clock.advance(600).index, 1);
  }
});

test("duration is endpoint span, including nonzero genesis and irregular final gap", () => {
  const clock = new PlaybackClock([100, 110, 123], 2);
  assert.equal(clock.horizon, 23);
  assert.equal(clock.advance(0).durationSeconds, 11.5);
  clock.seek(120.5, 0);
  assert.equal(clock.advance(0).remainingSeconds, 1.25);
  assert.equal(defaultPlaybackRate([100, 110, 123]), 23 / 120);
  assert.equal(defaultPlaybackRate([0, Number.MIN_VALUE]), Number.MIN_VALUE);
});
