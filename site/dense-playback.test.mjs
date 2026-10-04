import assert from "node:assert/strict";
import test from "node:test";
import { DensePlaybackSession, PlaybackClock, defaultPlaybackRate, sampleIndexAt } from "./playback.mjs";

test("миллион тиков использует компактный clock, dt=30 и точные границы", () => {
  const descriptor = { steps: 1_000_000, dt_seconds: 30 };
  const clock = new PlaybackClock(descriptor, 1);
  assert.equal(clock.count, 1_000_001); assert.equal(clock.endTime, 30_000_000);
  assert.equal(defaultPlaybackRate(descriptor), 250_000);
  assert.equal(Object.values(clock).some(Array.isArray), false);
  descriptor.steps = 2; descriptor.dt_seconds = 1;
  assert.equal(clock.count, 1_000_001); assert.equal(clock.timeAt(992), 29_760);
  for (const [time, tick] of [[0, 0], [29.999, 0], [30, 1], [29_759.999, 991], [29_760, 992], [29_999_999, 999_999], [30_000_000, 1_000_000]]) {
    assert.equal(clock.seek(time, 0).index, tick);
  }
  clock.seek(0, 0); clock.start(0);
  assert.equal(clock.advance(1000).playhead, 1); assert.equal(clock.advance(1000).index, 0);
  clock.setRate(30, 1000); assert.equal(clock.advance(2000).playhead, 31);
  const decimal = { steps: 10, dt_seconds: .1 };
  assert.equal(sampleIndexAt(decimal, 3 * .1), 3);
  assert.equal(sampleIndexAt(decimal, 3 * .1 - Number.EPSILON), 2);
});

test("uniform descriptor отклоняет неверные величины и не создаёт вымышленные ticks", () => {
  for (const descriptor of [{}, { steps: "10", dt_seconds: 30 }, { steps: -1, dt_seconds: 30 }, { steps: 1.5, dt_seconds: 30 }, { steps: 1_000_001, dt_seconds: 30 }, { steps: 10, dt_seconds: "30" }, { steps: 10, dt_seconds: 0 }, { steps: 10, dt_seconds: Infinity }, { steps: 10, dt_seconds: Number.MAX_VALUE }]) assert.throws(() => new PlaybackClock(descriptor), RangeError);
  const clock = new PlaybackClock({ steps: 0, dt_seconds: 30 });
  assert.equal(clock.count, 1); assert.equal(clock.start(0).playing, false);
  for (const tick of [-1, 1, .5, "0"]) assert.throws(() => clock.timeAt(tick), RangeError);
});

// Управляемые ответы границы decoder: тестируют async/clock, не биологию.
function controlled() {
  let now = 0; const requests = [], commits = [], statuses = [];
  const clock = new PlaybackClock({ steps: 10, dt_seconds: 30 }, 30);
  const session = new DensePlaybackSession(clock, (tick, { signal }) => new Promise((resolve, reject) => requests.push({ tick, signal, resolve, reject })), {
    now: () => now, commit: (frame, snapshot) => commits.push({ frame, ...snapshot }), status: (...args) => statuses.push(args),
  });
  const resolve = (index, tick = requests[index].tick) => requests[index].resolve({ frame: { tick, sim_time: tick * 30 } });
  return { session, clock, requests, commits, statuses, resolve, at: (time) => { now = time; } };
}
const settle = async () => { await Promise.resolve(); await Promise.resolve(); };
async function initialized() { const task = controlled(); const pending = task.session.seek(0); task.resolve(0); assert.equal(await pending, true); return task; }

test("buffering держит прежний подтверждённый кадр и исключает задержку из clock", async () => {
  const task = await initialized(); const { session, clock } = task;
  session.start(); task.at(1100); session.advance(1100);
  assert.equal(task.requests[1].tick, 1); assert.equal(session.buffering, true); assert.equal(session.playing, true);
  assert.equal(session.current.tick, 0); assert.equal(session.snapshot.playhead, 0);
  task.at(90_000); session.advance(90_000); assert.equal(task.requests.length, 2);
  assert.equal(clock.advance(90_000).playhead, 0);
  task.resolve(1); await settle();
  assert.equal(session.current.tick, 1); assert.equal(session.snapshot.playhead, 33); assert.equal(session.playing, true);
  task.at(90_200); session.advance(90_200);
  assert.equal(session.snapshot.playhead, 39); assert.equal(task.requests.length, 2);
  assert.equal(task.commits.at(-1).frame.tick, task.commits.at(-1).index);
});

test("новый seek, Pause и скрытая вкладка отвергают поздние ответы", async () => {
  const task = await initialized(); const { session } = task;
  const oldSeek = session.seek(60); const newSeek = session.seek(150.5);
  assert.equal(task.requests[1].signal.aborted, true);
  task.resolve(2); assert.equal(await newSeek, true); task.resolve(1); assert.equal(await oldSeek, false);
  assert.equal(session.current.tick, 5); assert.equal(session.snapshot.playhead, 150.5);
  session.start(); task.at(2000); session.advance(2000); assert.equal(session.buffering, true);
  session.pause("Paused while tab was hidden"); assert.equal(task.requests[3].signal.aborted, true);
  task.at(600_000); task.resolve(3); await settle();
  assert.equal(session.current.tick, 5); assert.equal(session.snapshot.playhead, 150.5); assert.equal(session.playing, false);
  assert.equal(task.statuses.at(-1)[0], "Paused while tab was hidden");
  session.start(); task.at(600_100); session.advance(600_100);
  assert.equal(session.snapshot.playhead, 153.5); assert.equal(task.requests.length, 4);
});

test("Next/Prev идут по одному tick, end/Replay подтверждают кадр до замены", async () => {
  const task = await initialized(); const { session } = task;
  let pending = session.step(1); assert.equal(task.requests[1].tick, 1); task.resolve(1); await pending;
  pending = session.step(-1); assert.equal(task.requests[2].tick, 0); task.resolve(2); await pending;
  pending = session.seek(300); task.resolve(3); await pending;
  assert.equal(session.current.tick, 10); assert.equal(session.snapshot.ended, true);
  session.start(); assert.equal(session.current.tick, 10); assert.equal(task.requests[4].tick, 0);
  task.resolve(4); await settle(); assert.equal(session.current.tick, 0); assert.equal(session.playing, true);
  assert.throws(() => session.step("1"), RangeError);
});

test("ошибка decoder и несоответствующий кадр сохраняют последнее состояние и останавливают clock", async () => {
  for (const wrongFrame of [false, true]) {
    const task = await initialized(); const { session } = task; const previous = session.current;
    const pending = session.seek(60);
    if (wrongFrame) task.resolve(1, 3); else task.requests[1].reject(new Error("Chunk digest mismatch"));
    assert.equal(await pending, false); assert.equal(session.current, previous); assert.equal(session.snapshot.index, 0);
    assert.equal(session.playing, false); assert.equal(task.statuses.at(-1)[0], "error");
    assert.match(session.failure.message, /digest|requested tick/);
    session.start(); session.advance(60_000); assert.equal(await session.seek(90), false); assert.equal(task.requests.length, 2);
  }
});

test("смена скорости во время загрузки отменяет старый seek и сохраняет явный playing intent", async () => {
  const task = await initialized(); const { session } = task;
  session.start(); task.at(1100); session.advance(1100); session.setRate(2);
  assert.equal(task.requests[1].signal.aborted, true); assert.equal(session.playing, true); assert.equal(session.snapshot.rate, 2);
  task.resolve(1); await settle(); assert.equal(session.current.tick, 0);
  task.at(2100); session.advance(2100); assert.equal(session.snapshot.playhead, 2);
  session.close(); assert.equal(session.playing, false);
});
