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
  if (!Array.isArray(times) || !times.length) throw new RangeError("Playback requires recorded sample times.");
  for (let index = 0; index < times.length; index++) {
    const time = times[index];
    if (typeof time !== "number" || !Number.isFinite(time) || time < 0 || (index > 0 && time <= times[index - 1])) {
      throw new RangeError("Recorded sample times must be finite, nonnegative, and strictly increasing.");
    }
  }
}

function overviewRate(times) {
  const horizon = times.at(-1) - times[0];
  return horizon === 0 ? 1 : Math.max(Number.MIN_VALUE, horizon / 120);
}

// A two-minute overview depends on the true model horizon, never sample count.
export function defaultPlaybackRate(times) {
  validateTimes(times);
  return overviewRate(times);
}

// Times are validated when the clock is constructed. Select only real samples.
export function sampleIndexAt(times, modelTime) {
  let low = 0, high = times.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (times[middle] <= modelTime) low = middle + 1;
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
    this._times = Object.freeze([...times]);
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
  get firstTime() { return this._times[0]; }
  get endTime() { return this._times.at(-1); }
  get horizon() { return this.endTime - this.firstTime; }

  _snapshot() {
    const index = sampleIndexAt(this._times, this._playhead);
    return {
      playhead: this._playhead,
      index,
      stateTime: this._times[index],
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
    const index = Math.max(0, Math.min(this._times.length - 1, current.index + offset));
    return this.seek(this._times[index], now);
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
