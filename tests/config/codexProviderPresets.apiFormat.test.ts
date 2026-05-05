import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "@/config/codexProviderPresets";

describe("codexProviderPresets apiFormat defaults", () => {
  it("defaults DeepSeek/Kimi/GLM to openai_chat", () => {
    const names = ["DeepSeek", "Kimi", "Zhipu GLM"];
    for (const name of names) {
      const preset = codexProviderPresets.find((p) => p.name === name);
      expect(preset).toBeTruthy();
      expect(preset?.apiFormat).toBe("openai_chat");
    }
  });
});
