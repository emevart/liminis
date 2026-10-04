export function validFrameRate(value) {
  const rate = Number(value);
  return Number.isFinite(rate) && rate >= 0.05 && rate <= 60;
}

export function playbackIndex(anchorIndex, elapsedMs, framesPerSecond, frameCount) {
  const elapsedFrames = Math.floor(Math.max(0, elapsedMs) * framesPerSecond / 1000);
  return Math.min(frameCount - 1, anchorIndex + elapsedFrames);
}
