import { PlayerObject } from "skinview3d";
import { Vector3 } from "three";
import { describe, expect, it } from "vitest";

import { createEmotecraftPreviewAnimation } from "./emotecraftPreview";

describe("Emotecraft skin preview", () => {
  it("applies legacy torso motion to the whole player, not the chest mesh", () => {
    const player = new PlayerObject();
    const animation = createEmotecraftPreviewAnimation();
    animation.fn(player, 0, 0);
    animation.fn(player, 1.65, 0);

    expect(Math.abs(player.rotation.z)).toBeGreaterThan(.05);
    expect(player.skin.body.rotation.z).toBe(0);
    expect(player.skin.position.y).toBe(8);
    expect(player.cape.position.y).toBe(8);
    player.updateMatrixWorld(true);
    const capeOffset = player.cape.getWorldPosition(new Vector3()).sub(player.skin.getWorldPosition(new Vector3()));
    expect(capeOffset.length()).toBeCloseTo(2);
    expect(player.position.length()).toBeLessThan(30);
  });

  it("keeps every clip finite while cycling through the extended sequence", () => {
    const player = new PlayerObject();
    const animation = createEmotecraftPreviewAnimation();
    for (let progress = 0; progress <= 36; progress += .25) {
      animation.fn(player, progress, .25);
      expect(player.position.toArray().every(Number.isFinite)).toBe(true);
      expect(player.quaternion.toArray().every(Number.isFinite)).toBe(true);
      expect(player.skin.position.y).toBe(8);
      expect(player.cape.position.y).toBe(8);
      expect(player.position.length()).toBeLessThan(35);
    }
  });
});
