import { SkinObject } from "skinview3d";
import { BufferAttribute, Mesh } from "three";
import { describe, expect, it } from "vitest";

import { installBendableLimbs, resetLimbBends, setLimbBend } from "./bendableSkin";

describe("bendable skin preview", () => {
  it("keeps the original skin meshes and their UV map while bending", () => {
    const skin = new SkinObject();
    skin.modelType = "default";
    const inner = skin.rightArm.innerLayer as Mesh;
    const originalUvs = new Set(Array.from((inner.geometry.attributes.uv as BufferAttribute).array).map((value) => Number(value).toFixed(6)));
    installBendableLimbs(skin);

    expect(skin.rightArm.innerLayer.visible).toBe(true);
    expect(skin.rightArm.outerLayer.visible).toBe(true);
    expect(skin.rightArm.children.flatMap((child) => child.children).filter((child) => child instanceof Mesh)).toHaveLength(2);
    const bentUvs = new Set(Array.from((inner.geometry.attributes.uv as BufferAttribute).array).map((value) => Number(value).toFixed(6)));
    for (const uv of originalUvs) expect(bentUvs.has(uv)).toBe(true);

    const positions = inner.geometry.attributes.position as BufferAttribute;
    const straight = Array.from(positions.array);

    setLimbBend(skin, "rightArm", -.62);
    expect(Array.from(positions.array)).not.toEqual(straight);
    expect(inner.rotation.x).toBe(0);

    resetLimbBends(skin);
    expect(Array.from(positions.array)).toEqual(straight);
  });

  it("rebuilds the original UV-preserving mesh when switching to slim arms", () => {
    const skin = new SkinObject();
    installBendableLimbs(skin);
    skin.modelType = "slim";
    installBendableLimbs(skin);

    const inner = skin.rightArm.innerLayer as Mesh;
    inner.geometry.computeBoundingBox();
    expect(inner.geometry.boundingBox!.max.x - inner.geometry.boundingBox!.min.x).toBeCloseTo(3);
    setLimbBend(skin, "rightArm", -.5, Math.PI / 4);
    expect(Array.from((inner.geometry.attributes.position as BufferAttribute).array).every(Number.isFinite)).toBe(true);
  });
});
