import { FunctionAnimation } from "skinview3d";
import { Euler, Quaternion, Vector3 } from "three";

import bow from "../../assets/emotes/bow.json";
import cheer from "../../assets/emotes/cheer.json";
import extendArms from "../../assets/emotes/extend-arms.json";
import shrug from "../../assets/emotes/shrug.json";
import wave from "../../assets/emotes/wave.json";
import yes from "../../assets/emotes/yes.json";
import { installBendableLimbs, resetLimbBends, setLimbBend, type BendablePart } from "./bendableSkin";

type PartName = "body" | "head" | "torso" | "rightArm" | "leftArm" | "rightLeg" | "leftLeg";
type ChannelName = "x" | "y" | "z" | "pitch" | "yaw" | "roll" | "axis" | "bend";
type EaseName = "LINEAR" | "CONSTANT" | "EASEINQUAD" | "EASEINOUTQUAD" | "EASEINSINE";
type Tracks = Record<PartName, Partial<Record<ChannelName, Keyframe[]>>>;

interface Keyframe { tick: number; value: number; easing: EaseName; }
interface RawMove { tick: number; easing?: EaseName; [part: string]: unknown; }
interface RawEmoteFile { version?: number; emote: { easeBeforeKeyframe?: boolean | string; endTick: number; moves: RawMove[] }; }
interface PreviewClip { duration: number; easeBeforeKeyframe: boolean; tracks: Tracks; }

const parts: PartName[] = ["body", "head", "torso", "rightArm", "leftArm", "rightLeg", "leftLeg"];
const channels: ChannelName[] = ["x", "y", "z", "pitch", "yaw", "roll", "axis", "bend"];
const emoteBindPosition: Record<PartName, [number, number, number]> = {
  body: [0, 0, 0], head: [0, 0, 0], torso: [0, 0, 0], rightArm: [-5, 2, 0], leftArm: [5, 2, 0], rightLeg: [-1.9, 12, .1], leftLeg: [1.9, 12, .1],
};
const viewerBindPosition: Record<PartName, [number, number, number]> = {
  body: [0, 0, 0], head: [0, 0, 0], torso: [0, -6, 0], rightArm: [-5, -2, 0], leftArm: [5, -2, 0], rightLeg: [-1.9, -12, -.1], leftLeg: [1.9, -12, -.1],
};
const clips = [wave, cheer, bow, shrug, extendArms, yes].map((source) => compileClip(source as RawEmoteFile));

export function createEmotecraftPreviewAnimation() {
  let clipIndex = 0;
  let clipStartedAt = 0;
  let initialized = false;
  return new FunctionAnimation((player, progress) => {
    if (!initialized) {
      initialized = true;
      clipStartedAt = progress + .45;
    }

    const clip = clips[clipIndex];
    const local = progress - clipStartedAt;
    if (local > clip.duration + 1.35) {
      clipIndex = (clipIndex + 1) % clips.length;
      clipStartedAt = progress + .55;
    }

    player.resetJoints();
    player.position.set(0, 0, 0);
    player.quaternion.identity();
    installBendableLimbs(player.skin);
    resetLimbBends(player.skin);
    const idle = Math.cos(progress * 1.8) * .025;
    player.skin.leftArm.rotation.z = Math.PI * .02 + idle;
    player.skin.rightArm.rotation.z = -Math.PI * .02 - idle;
    player.skin.body.rotation.y = Math.sin(progress * .7) * .012;
    const idleBob = Math.sin(progress * 1.8) * .08;
    player.position.y = idleBob;
    player.cape.rotation.x = Math.PI * .06 + Math.sin(progress * 1.8) * .012;
    if (local < 0) return;

    if (local > clip.duration + .28) return;
    const fadeIn = smoothStep(Math.min(1, local / .16));
    const fadeOut = local <= clip.duration ? 1 : 1 - smoothStep((local - clip.duration) / .28);
    const weight = fadeIn * fadeOut;
    applyClip(player, clip, Math.min(local * 20, clip.duration * 20), weight);
    player.position.y += idleBob;
  });
}

function compileClip(source: RawEmoteFile): PreviewClip {
  const tracks = Object.fromEntries(parts.map((part) => [part, {}])) as Tracks;
  for (const move of source.emote.moves) {
    for (const part of parts) {
      // Emotecraft v1/v2 uses "torso" for the whole-body root. Only v3 has
      // an independently animated chest. Rotating skin.body for old clips
      // pulls the shoulders away from the head and makes the torso look bent.
      if (part === "torso" && (source.version ?? 1) < 3) continue;
      const values = move[part === "body" && (source.version ?? 1) < 3 ? "torso" : part];
      if (!values || typeof values !== "object") continue;
      for (const channel of channels) {
        const value = (values as Record<string, unknown>)[channel];
        if (typeof value !== "number") continue;
        (tracks[part][channel] ??= []).push({ tick: move.tick, value, easing: move.easing ?? "LINEAR" });
      }
    }
  }
  for (const part of parts) for (const channel of channels) tracks[part][channel]?.sort((a, b) => a.tick - b.tick);
  return { duration: source.emote.endTick / 20, easeBeforeKeyframe: source.emote.easeBeforeKeyframe === true || source.emote.easeBeforeKeyframe === "true", tracks };
}

function applyClip(player: Parameters<FunctionAnimation["fn"]>[0], clip: PreviewClip, tick: number, weight: number) {
  const skin = player.skin;
  applyRoot(player, clip, tick, weight);
  for (const part of parts) {
    if (part === "body") continue;
    const object = part === "torso" ? skin.body : skin[part];
    const tracks = clip.tracks[part];
    const value = (channel: ChannelName, initial = 0) => sample(tracks[channel], tick, initial, clip.easeBeforeKeyframe);
    const target = new Quaternion().setFromEuler(new Euler(
      value("pitch"),
      -value("yaw"),
      -value("roll"),
      "ZYX",
    ));
    object.quaternion.identity().slerp(target, weight);
    const emoteBase = emoteBindPosition[part];
    const viewerBase = viewerBindPosition[part];
    object.position.x = viewerBase[0] + (value("x", emoteBase[0]) - emoteBase[0]) * weight;
    object.position.y = viewerBase[1] - (value("y", emoteBase[1]) - emoteBase[1]) * weight;
    object.position.z = viewerBase[2] - (value("z", emoteBase[2]) - emoteBase[2]) * weight;
    if (part !== "head" && part !== "torso") {
      setLimbBend(
        skin,
        part as BendablePart,
        value("bend") * weight,
        value("axis"),
      );
    }
  }
}

function applyRoot(player: Parameters<FunctionAnimation["fn"]>[0], clip: PreviewClip, tick: number, weight: number) {
  const tracks = clip.tracks.body;
  const value = (channel: ChannelName) => sample(tracks[channel], tick, 0, clip.easeBeforeKeyframe);
  const target = new Quaternion().setFromEuler(new Euler(
    -value("pitch"),
    value("yaw"),
    -value("roll"),
    "ZYX",
  ));
  player.quaternion.identity().slerp(target, weight);
  // skinview3d places skin and cape at y=8 inside PlayerObject; its feet are
  // y=-16. The Emotecraft root pivot is 11.2 units above the feet, at -4.8.
  // Moving PlayerObject keeps the cape attached and leaves skin's bind pose intact.
  const pivot = new Vector3(0, -4.8, 0);
  player.position.copy(pivot).sub(pivot.clone().applyQuaternion(player.quaternion));
  player.position.add(new Vector3(
    -16 * value("x"),
    16 * value("y"),
    -16 * value("z"),
  ).multiplyScalar(weight));
}

function sample(track: Keyframe[] | undefined, tick: number, initial: number, easeBeforeKeyframe: boolean) {
  if (!track?.length) return initial;
  let previous: Keyframe = { tick: 0, value: initial, easing: "LINEAR" };
  for (const next of track) {
    if (tick <= next.tick) {
      if (next.tick === previous.tick) return next.value;
      const progress = (tick - previous.tick) / (next.tick - previous.tick);
      return previous.value + (next.value - previous.value) * ease(easeBeforeKeyframe ? next.easing : previous.easing, Math.max(0, Math.min(1, progress)));
    }
    previous = next;
  }
  return previous.value;
}

function ease(name: EaseName, value: number) {
  if (name === "CONSTANT") return value >= 1 ? 1 : 0;
  if (name === "EASEINQUAD") return value * value;
  if (name === "EASEINOUTQUAD") return value < .5 ? 2 * value * value : 1 - Math.pow(-2 * value + 2, 2) / 2;
  if (name === "EASEINSINE") return 1 - Math.cos(value * Math.PI / 2);
  return value;
}

function smoothStep(value: number) { return value * value * (3 - 2 * value); }
