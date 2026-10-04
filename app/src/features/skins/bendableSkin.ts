import type { BodyPart, SkinObject } from "skinview3d";
import { BufferAttribute, type BufferGeometry, Matrix4, Mesh, Vector3 } from "three";
import { TessellateModifier } from "three/examples/jsm/modifiers/TessellateModifier.js";

export type BendablePart = "rightArm" | "leftArm" | "rightLeg" | "leftLeg";

interface BendMesh {
  base: Float32Array;
  geometry: BufferGeometry;
  halfHeight: number;
  mesh: Mesh;
}

interface BendRig {
  inner: BendMesh;
  lastAngle: number;
  lastAxis: number;
  modelType: SkinObject["modelType"];
  outer: BendMesh;
}

const rigs = new WeakMap<BodyPart, BendRig>();
const sourceGeometries = new WeakMap<Mesh, BufferGeometry>();

export function installBendableLimbs(skin: SkinObject) {
  installLimb(skin.rightArm, skin.modelType);
  installLimb(skin.leftArm, skin.modelType);
  installLimb(skin.rightLeg, skin.modelType);
  installLimb(skin.leftLeg, skin.modelType);
}

export function resetLimbBends(skin: SkinObject) {
  for (const name of ["rightArm", "leftArm", "rightLeg", "leftLeg"] as BendablePart[]) {
    applyBend(rigs.get(skin[name]), 0, 0);
  }
}

export function setLimbBend(skin: SkinObject, name: BendablePart, angle: number, axis = 0) {
  applyBend(rigs.get(skin[name]), axis, angle);
}

function installLimb(part: BodyPart, modelType: SkinObject["modelType"]) {
  const current = rigs.get(part);
  if (current?.modelType === modelType) return;
  if (current) {
    current.inner.geometry.dispose();
    current.outer.geometry.dispose();
  }

  const innerMesh = part.innerLayer as Mesh;
  const outerMesh = part.outerLayer as Mesh;
  // Work directly from skinview3d's own meshes. Their proven Minecraft UV map is
  // retained byte-for-byte; only extra vertices are inserted for smooth bending.
  const inner = makeBendMesh(innerMesh);
  const outer = makeBendMesh(outerMesh);
  part.innerLayer.visible = true;
  part.outerLayer.visible = true;
  const rig = { inner, lastAngle: Number.NaN, lastAxis: Number.NaN, modelType, outer };
  rigs.set(part, rig);
  applyBend(rig, 0, 0);
}

function makeBendMesh(mesh: Mesh): BendMesh {
  let source = sourceGeometries.get(mesh);
  if (!source) {
    source = mesh.geometry;
    sourceGeometries.set(mesh, source);
  }
  const scale = mesh.scale.clone();
  const baked = source.clone().applyMatrix4(new Matrix4().makeScale(scale.x, scale.y, scale.z));
  // One-pixel triangles reproduce BendyLib's pixel-quad model while preserving
  // the exact UV attributes supplied by skinview3d.
  const geometry = new TessellateModifier(1.01, 8).modify(baked);
  baked.dispose();
  mesh.geometry = geometry;
  mesh.scale.set(1, 1, 1);
  const position = geometry.attributes.position as BufferAttribute;
  geometry.computeBoundingBox();
  const bounds = geometry.boundingBox!;
  return {
    base: new Float32Array(position.array as ArrayLike<number>),
    geometry,
    halfHeight: (bounds.max.y - bounds.min.y) / 2,
    mesh,
  };
}

// TypeScript port of BendyLib's IBendable.applyBend. The limb is one continuous
// cuboid; the nearer half rotates around its middle plane and both halves receive
// the same tangent compensation that closes the seam in Emotecraft.
function applyBend(rig: BendRig | undefined, axis: number, angle: number) {
  if (!rig || (rig.lastAxis === axis && rig.lastAngle === angle)) return;
  deform(rig.inner, axis, angle);
  deform(rig.outer, axis, angle);
  rig.lastAxis = axis;
  rig.lastAngle = angle;
}

function deform(target: BendMesh, bendAxis: number, bendValue: number) {
  const position = target.geometry.attributes.position as BufferAttribute;
  const values = position.array as Float32Array;
  const axisX = Math.cos(bendAxis);
  const axisZ = Math.sin(bendAxis);
  const sin = Math.sin(bendValue);
  const cos = Math.cos(bendValue);
  const tangent = Math.tan(bendValue / 2);
  const center = new Vector3(0, 0, 0);
  const rotationAxis = new Vector3(axisX, 0, axisZ);

  for (let index = 0; index < values.length; index += 3) {
    const originalX = target.base[index];
    const originalY = target.base[index + 1];
    const originalZ = target.base[index + 2];
    // For a downward limb the signed distance from the bend plane is the
    // horizontal component perpendicular to the chosen bend axis.
    const distanceFromBend = originalX * axisZ - originalZ * axisX;
    const compensation = tangent * distanceFromBend;
    let x = originalX;
    let y = originalY;
    let z = originalZ;

    if (originalY < 0) {
      y += ((originalY + target.halfHeight) / target.halfHeight) * compensation;
      const dot = x * rotationAxis.x + y * rotationAxis.y + z * rotationAxis.z;
      const crossX = -rotationAxis.z * y;
      const crossY = rotationAxis.z * x - rotationAxis.x * z;
      const crossZ = rotationAxis.x * y;
      x = x * cos + crossX * sin + rotationAxis.x * dot * (1 - cos) + center.x;
      y = y * cos + crossY * sin + rotationAxis.y * dot * (1 - cos) + center.y;
      z = z * cos + crossZ * sin + rotationAxis.z * dot * (1 - cos) + center.z;
    } else {
      y += ((originalY - target.halfHeight) / target.halfHeight) * compensation;
    }

    values[index] = x;
    values[index + 1] = y;
    values[index + 2] = z;
  }
  position.needsUpdate = true;
  target.geometry.computeVertexNormals();
  target.geometry.computeBoundingBox();
  target.geometry.computeBoundingSphere();
}
