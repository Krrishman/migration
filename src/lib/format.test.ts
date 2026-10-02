import { formatBytes, formatDuration, formatEta, percent, truncateMiddle } from "./format";

describe("formatBytes", () => {
  it("matches the Rust formatter", () => {
    expect(formatBytes(0)).toBe("0 bytes");
    expect(formatBytes(1)).toBe("1 byte");
    expect(formatBytes(1023)).toBe("1023 bytes");
    expect(formatBytes(1024)).toBe("1.00 KB");
    expect(formatBytes(1536)).toBe("1.50 KB");
    expect(formatBytes(10 * 1024 * 1024)).toBe("10.0 MB");
    expect(formatBytes(250 * 1024 ** 3)).toBe("250 GB");
    expect(formatBytes(null)).toBe("Unknown");
  });
});

describe("time formatting", () => {
  it("formats durations", () => {
    expect(formatDuration(5)).toBe("5 s");
    expect(formatDuration(65)).toBe("1 min 05 s");
    expect(formatDuration(3700)).toBe("1 h 01 min");
  });
  it("never claims an exact ETA it does not have", () => {
    expect(formatEta(null, false)).toBe("Estimating…");
    expect(formatEta(90, false)).toBe("About 1 min 30 s left");
    expect(formatEta(90, true)).toBe("—");
  });
});

it("truncates long paths in the middle", () => {
  const p = "C:\\Users\\ann\\Documents\\Projects\\Very\\Deep\\Folder\\Structure\\file.txt";
  const t = truncateMiddle(p, 30);
  expect(t.length).toBe(30);
  expect(t.startsWith("C:\\Users")).toBe(true);
  expect(t.endsWith("file.txt")).toBe(true);
});

it("computes bounded percentages", () => {
  expect(percent(5, 10)).toBe(50);
  expect(percent(15, 10)).toBe(100);
  expect(percent(1, null)).toBeNull();
  expect(percent(1, 0)).toBeNull();
});
