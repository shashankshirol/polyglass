// The Freeze mode snapshot as `freeze::rgba` sends it: width and height (u32, little-endian), then
// RGBA rows without padding (MODE-03).

export function snapshotSize(buf: ArrayBuffer): { width: number; height: number } {
  if (buf.byteLength < 8) throw new Error("snapshot too short");
  const view = new DataView(buf);
  const width = view.getUint32(0, true);
  const height = view.getUint32(4, true);
  if (buf.byteLength !== 8 + width * height * 4) throw new Error("snapshot size mismatch");
  return { width, height };
}

export function decodeSnapshot(buf: ArrayBuffer): ImageData {
  const { width, height } = snapshotSize(buf);
  return new ImageData(new Uint8ClampedArray(buf, 8), width, height);
}
