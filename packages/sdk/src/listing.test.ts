import { describe, expect, it } from "vitest";
import {
  canonicalListing,
  defaultDescription,
  defaultTags,
  defaultTitle,
  familyFallbackTitle,
  formatTags,
  listingHeadline,
  parseTagsInput,
} from "./listing";

describe("listing identity", () => {
  it("keeps CPI wording on the create preset only", () => {
    expect(defaultTitle(1)).toContain("US CPI YoY");
  });

  it("does not name an untitled gaussian board after the CPI example", () => {
    expect(listingHeadline({ title: "", family: 1, market: "Seed111111111111111111111111111111111111111" })).toBe(
      "Gaussian prediction market",
    );
    expect(familyFallbackTitle(0)).toBe("Football prediction market");
    expect(listingHeadline({ title: "  US CPI YoY persist check  ", family: 1, market: "x" })).toBe(
      "US CPI YoY persist check",
    );
  });

  it("parses several catalog tags for one market", () => {
    expect(parseTagsInput("football, epl")).toEqual(["football", "epl"]);
    expect(parseTagsInput("football，world cup | test")).toEqual(["football", "world cup", "test"]);
    expect(formatTags(["football", "epl"])).toBe("football · epl");
    expect(defaultTags(0)).toEqual(["football"]);
    expect(defaultDescription(0)).toContain("full-time");
  });

  it("canonicalListing prefers English fields over a translated desk view", () => {
    const canon = canonicalListing({
      market: "M",
      title: "美国 CPI",
      title_en: "US CPI YoY",
      event: "首次打印",
      event_en: "US CPI YoY first print",
      description: "中文说明",
      description_en: "First official print.",
      is_translation: true,
      locale: "zh-Hans",
    });
    expect(canon.title).toBe("US CPI YoY");
    expect(canon.event).toBe("US CPI YoY first print");
    expect(canon.description).toBe("First official print.");
  });
});
