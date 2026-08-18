import { describe, expect, test } from "bun:test";
import { LANGUAGES } from "../../src/constants/languages";

describe("LANGUAGES", () => {
  test("codes are unique", () => {
    const codes = LANGUAGES.map((lang) => lang.code);
    expect(new Set(codes).size).toBe(codes.length);
  });

  test("includes auto-detect, auto-translate and Chinese", () => {
    const codes = LANGUAGES.map((lang) => lang.code);
    expect(codes).toContain("auto");
    expect(codes).toContain("auto-translate");
    expect(codes).toContain("zh");
  });
});
