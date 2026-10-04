function finiteNumber(value) {
  if (typeof value !== "number" && (typeof value !== "string" || !value.trim())) return null;
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

// 1× means one recorded model second per wall second, with no rate ceiling.
export function validPlaybackRate(value) {
  const rate = finiteNumber(value);
  return rate !== null && rate > 0;
}

function validateTimes(times) {
  if (!Array.isArray(times)) {
    if (!times || !Number.isSafeInteger(times.steps) || times.steps < 0 || times.steps > 1_000_000 || typeof times.dt_seconds !== "number" || !Number.isFinite(times.dt_seconds) || times.dt_seconds <= 0 || !Number.isFinite(times.steps * times.dt_seconds)) throw new RangeError("Playback requires valid uniform steps/dt or recorded sample times.");
    return;
  }
  if (!Array.isArray(times) || !times.length) throw new RangeError("Playback requires recorded sample times.");
  for (let index = 0; index < times.length; index++) {
    const time = times[index];
    if (typeof time !== "number" || !Number.isFinite(time) || time < 0 || (index > 0 && time <= times[index - 1])) {
      throw new RangeError("Recorded sample times must be finite, nonnegative, and strictly increasing.");
    }
  }
}

function overviewRate(times) {
  const horizon = Array.isArray(times) ? times.at(-1) - times[0] : times.steps * times.dt_seconds;
  return horizon === 0 ? 1 : Math.max(Number.MIN_VALUE, horizon / 120);
}

// A two-minute overview depends on the true model horizon, never sample count.
export function defaultPlaybackRate(times) {
  validateTimes(times);
  return overviewRate(times);
}

// Times are validated when the clock is constructed. Select only real samples.
export function sampleIndexAt(times, modelTime) {
  let low = 0, high = Array.isArray(times) ? times.length : times.steps + 1;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if ((Array.isArray(times) ? times[middle] : middle * times.dt_seconds) <= modelTime) low = middle + 1;
    else high = middle;
  }
  return Math.max(0, low - 1);
}

function wallTime(now) {
  if (typeof now !== "number" || !Number.isFinite(now)) throw new RangeError("Wall time must be finite milliseconds.");
  return now;
}

export class PlaybackClock {
  constructor(times, rate) {
    validateTimes(times);
    this._times = Array.isArray(times) ? Object.freeze([...times]) : null;
    this._uniform = Array.isArray(times) ? null : Object.freeze({ steps: times.steps, dt_seconds: times.dt_seconds });
    const chosenRate = rate === undefined ? overviewRate(times) : rate;
    if (!validPlaybackRate(chosenRate)) throw new RangeError("Playback rate must be positive and finite.");
    this._rate = Number(chosenRate);
    this._playing = false;
    this._playhead = this.firstTime;
    this._anchorModel = this.firstTime;
    this._anchorWall = 0;
  }

  get rate() { return this._rate; }
  get playing() { return this._playing; }
  get count() { return this._times ? this._times.length : this._uniform.steps + 1; }
  timeAt(index) { if (!Number.isSafeInteger(index) || index < 0 || index >= this.count) throw new RangeError("Sample index is outside the recording."); return this._times ? this._times[index] : index * this._uniform.dt_seconds; }
  get firstTime() { return this.timeAt(0); }
  get endTime() { return this.timeAt(this.count - 1); }
  get horizon() { return this.endTime - this.firstTime; }

  _snapshot() {
    const index = sampleIndexAt(this._times ?? this._uniform, this._playhead);
    return {
      playhead: this._playhead,
      index,
      stateTime: this.timeAt(index),
      rate: this.rate,
      playing: this.playing,
      ended: this._playhead === this.endTime,
      durationSeconds: this.horizon / this.rate,
      remainingSeconds: (this.endTime - this._playhead) / this.rate,
    };
  }

  advance(now) {
    wallTime(now);
    if (this.playing) {
      const elapsedMs = now - this._anchorWall;
      const elapsedSeconds = Math.max(0, Number.isFinite(elapsedMs) ? elapsedMs / 1000 : now / 1000 - this._anchorWall / 1000);
      const remainingSeconds = (this.endTime - this._anchorModel) / this.rate;
      // Compare before multiplying: huge finite rates and late draws can overflow.
      this._playhead = elapsedSeconds === 0 ? this._anchorModel : elapsedSeconds >= remainingSeconds
        ? this.endTime
        : Math.min(this.endTime, this._anchorModel + elapsedSeconds * this.rate);
      if (this._playhead === this.endTime) this._playing = false;
    }
    return this._snapshot();
  }

  start(now) {
    wallTime(now);
    if (this.playing) return this.advance(now);
    if (this._playhead === this.endTime) this._playhead = this.firstTime;
    this._anchorModel = this._playhead;
    this._anchorWall = now;
    this._playing = this.horizon > 0;
    return this._snapshot();
  }

  pause(now) {
    this.advance(now);
    this._playing = false;
    this._anchorModel = this._playhead;
    this._anchorWall = now;
    return this._snapshot();
  }

  seek(modelTime, now) {
    const time = finiteNumber(modelTime);
    if (time === null) throw new RangeError("Seek time must be finite model seconds.");
    wallTime(now);
    this._playing = false;
    this._playhead = Math.max(this.firstTime, Math.min(this.endTime, time));
    this._anchorModel = this._playhead;
    this._anchorWall = now;
    return this._snapshot();
  }

  step(offset, now) {
    if (!Number.isSafeInteger(offset)) throw new RangeError("Sample step must be an integer.");
    const current = this.pause(now);
    const index = Math.max(0, Math.min(this.count - 1, current.index + offset));
    return this.seek(this.timeAt(index), now);
  }

  setRate(value, now) {
    if (!validPlaybackRate(value)) throw new RangeError("Playback rate must be positive and finite.");
    this.advance(now);
    this._rate = Number(value);
    this._anchorModel = this._playhead;
    this._anchorWall = now;
    return this._snapshot();
  }
}

// Один показанный кадр и отменяемый seek. Время загрузки не входит в playback.
export class DensePlaybackSession {
  constructor(clock, seekFrame, { now = () => performance.now(), commit = () => {}, status = () => {} } = {}) {
    this.clock = clock; this.seekFrame = seekFrame; this.now = now; this.commit = commit; this.status = status;
    this.current = null; this.snapshot = clock.advance(now()); this.buffering = false; this.failure = null;
    this._serial = 0; this._controller = null; this._resume = false; this._closed = false;
  }
  get playing() { return this.clock.playing || (this.buffering && this._resume); }
  _emit(snapshot) { this.snapshot = snapshot; if (this.current) this.commit(this.current, snapshot); }
  _cancel() { this._serial++; this._controller?.abort(); this._controller = null; this.buffering = false; this._resume = false; }
  fail(error) {
    if (this.failure || this._closed) return;
    this.failure = error; this._cancel();
    this.snapshot = this.clock.seek(this.snapshot.playhead, this.now());
    this.status("error", error);
  }
  async _request(target, resume) {
    this._cancel(); const serial = this._serial, controller = new AbortController(); this._controller = controller;
    this.buffering = true; this._resume = resume && !target.ended;
    this.clock.seek(this.snapshot.playhead, this.now()); this.status("buffering", target.index);
    try {
      const decoded = await this.seekFrame(target.index, { signal: controller.signal });
      if (serial !== this._serial || controller.signal.aborted) return false;
      if (decoded?.frame?.tick !== target.index || decoded.frame.sim_time !== target.stateTime) throw new Error("Dense frame does not match the requested tick/model time.");
      const resumeAfter = this._resume; this.buffering = false; this._controller = null; this._resume = false;
      const now = this.now(); this.clock.seek(target.playhead, now); if (resumeAfter) this.clock.start(now);
      this.current = decoded.frame; this._emit(this.clock.advance(now)); this.status(this.snapshot.ended ? "ended" : this.playing ? "playing" : "paused"); return true;
    } catch (error) {
      if (serial !== this._serial || controller.signal.aborted) return false;
      this.fail(error); return false;
    }
  }
  seek(time) { if (this.failure || this._closed) return Promise.resolve(false); return this._request(this.clock.seek(time, this.now()), false); }
  step(offset) { if (!Number.isSafeInteger(offset)) throw new RangeError("Sample step must be an integer."); return this.seek(this.clock.timeAt(Math.max(0, Math.min(this.clock.count - 1, this.snapshot.index + offset)))); }
  start() {
    if (this.failure || this._closed || this.buffering) return;
    const target = this.clock.start(this.now());
    if (!this.current || target.index !== this.snapshot.index) { void this._request(target, target.playing); return; }
    this._emit(target); this.status(target.ended ? "ended" : "playing");
  }
  advance(now) {
    if (this.failure || this._closed || this.buffering || !this.clock.playing) return;
    const target = this.clock.advance(now);
    if (target.index !== this.snapshot.index) { void this._request(target, target.playing); return; }
    this._emit(target); if (target.ended) this.status("ended");
  }
  pause(message = "paused") {
    if (this.failure || this._closed) return;
    const wasBuffering = this.buffering; this._cancel();
    if (wasBuffering) this.clock.seek(this.snapshot.playhead, this.now());
    else {
      const target = this.clock.pause(this.now());
      // Pause не запрашивает ещё не загруженное наблюдение.
      this.clock.seek(target.index === this.snapshot.index ? target.playhead : this.snapshot.playhead, this.now());
    }
    this._emit(this.clock.advance(this.now())); this.status(message);
  }
  setRate(rate) {
    if (this.failure || this._closed) return;
    if (!validPlaybackRate(rate)) throw new RangeError("Playback rate must be positive and finite.");
    const playing = this.playing; this.pause(); this.clock.setRate(rate, this.now());
    if (playing) this.start(); else this._emit(this.clock.advance(this.now()));
  }
  close() { this._closed = true; this._cancel(); this.snapshot = this.clock.seek(this.snapshot.playhead, this.now()); }
}
