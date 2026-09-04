import { FunctionAnimation } from "skinview3d";

import bow from "../../assets/emotes/bow.json";
import extendArms from "../../assets/emotes/extend-arms.json";
import yes from "../../assets/emotes/yes.json";

type PartName = "head" | "torso" | "rightArm" | "leftArm" | "rightLeg" | "leftLeg";
type ChannelName = "x" | "y" | "z" | "pitch" | "yaw" | "roll";
type EaseName = "LINEAR" | "CONSTANT" | "EASEINQUAD" | "EASEINOUTQUAD" | "EASEINSINE";
type Tracks = Record<PartName, Partial<Record<ChannelName, Keyframe[]>>>;

interface Keyframe { tick: number; value: number; easing: EaseName; }
interface RawMove { tick: number; easing?: EaseName; [part: string]: unknown; }
interface RawEmoteFile { emote: { endTick: number; moves: RawMove[] }; }
interface PreviewClip { duration: number; tracks: Tracks; }

const parts: PartName[] = ["head", "torso", "rightArm", "leftArm", "rightLeg", "leftLeg"];
const channels: ChannelName[] = ["x", "y", "z", "pitch", "yaw", "roll"];
const emoteBindPosition: Record<PartName, [number, number, number]> = {
  head: [0, 0, 0], torso: [0, 0, 0], rightArm: [-5, 2, 0], leftArm: [5, 2, 0], rightLeg: [-1.9, 12, .1], leftLeg: [1.9, 12, .1],
};
const viewerBindPosition: Record<PartName, [number, number, number]> = {
  head: [0, 0, 0], torso: [0, -6, 0], rightArm: [-5, -2, 0], leftArm: [5, -2, 0], rightLeg: [-1.9, -12, -.1], leftLeg: [1.9, -12, -.1],
};
const clips = [yes, bow, extendArms].map((source) => compileClip(source as RawEmoteFile));

export function createEmotecraftPreviewAnimation() {
  let cycle = 0;
  let clipIndex = -1;
  return new FunctionAnimation((player, progress) => {
    const nextCycle = Math.floor(progress / 8);
    if (nextCycle !== cycle) {
      cycle = nextCycle;
      const available = clips.map((_, index) => index).filter((index) => index !== clipIndex);
      clipIndex = available[Math.floor(Math.random() * available.length)];
    }

    player.skin.resetJoints();
    const idle = Math.cos(progress * 2) * .03;
    player.skin.leftArm.rotation.z = Math.PI * .02 + idle;
    player.skin.rightArm.rotation.z = -Math.PI * .02 - idle;
    player.cape.rotation.x = Math.PI * .06 + Math.sin(progress * 2) * .01;
    if (cycle === 0 || clipIndex < 0) return;

    const clip = clips[clipIndex];
    const local = progress % 8;
    if (local > clip.duration + .2) return;
    const weight = local <= clip.duration ? 1 : 1 - smoothStep((local - clip.duration) / .2);
    applyClip(player.skin, clip, Math.min(local * 20, clip.duration * 20), weight);
  });
}

function compileClip(source: RawEmoteFile): PreviewClip {
  const tracks = Object.fromEntries(parts.map((part) => [part, {}])) as Tracks;
  for (const move of source.emote.moves) {
    for (const part of parts) {
      const values = move[part];
      if (!values || typeof values !== "object") continue;
      for (const channel of channels) {
        const value = (values as Record<string, unknown>)[channel];
        if (typeof value !== "number") continue;
        (tracks[part][channel] ??= []).push({ tick: move.tick, value, easing: move.easing ?? "LINEAR" });
      }
    }
  }
  for (const part of parts) for (const channel of channels) tracks[part][channel]?.sort((a, b) => a.tick - b.tick);
  return { duration: source.emote.endTick / 20, tracks };
}

function applyClip(skin: Parameters<FunctionAnimation["fn"]>[0]["skin"], clip: PreviewClip, tick: number, weight: number) {
  for (const part of parts) {
    const object = part === "torso" ? skin.body : skin[part];
    const tracks = clip.tracks[part];
    object.rotation.x = sample(tracks.pitch, tick, 0) * weight;
    object.rotation.y = sample(tracks.yaw, tick, 0) * weight;
    object.rotation.z = sample(tracks.roll, tick, 0) * weight;
    const emoteBase = emoteBindPosition[part];
    const viewerBase = viewerBindPosition[part];
    object.position.x = viewerBase[0] + (sample(tracks.x, tick, emoteBase[0]) - emoteBase[0]) * weight;
    object.position.y = viewerBase[1] - (sample(tracks.y, tick, emoteBase[1]) - emoteBase[1]) * weight;
    object.position.z = viewerBase[2] - (sample(tracks.z, tick, emoteBase[2]) - emoteBase[2]) * weight;
  }
}

function sample(track: Keyframe[] | undefined, tick: number, initial: number) {
  if (!track?.length) return initial;
  let previous: Keyframe = { tick: 0, value: initial, easing: "LINEAR" };
  for (const next of track) {
    if (tick <= next.tick) {
      if (next.tick === previous.tick) return next.value;
      const progress = (tick - previous.tick) / (next.tick - previous.tick);
      return previous.value + (next.value - previous.value) * ease(next.easing, Math.max(0, Math.min(1, progress)));
    }
    previous = next;
  }
  return previous.value;
}

function ease(name: EaseName, value: number) {
  if (name === "CONSTANT") return 0;
  if (name === "EASEINQUAD") return value * value;
  if (name === "EASEINOUTQUAD") return value < .5 ? 2 * value * value : 1 - Math.pow(-2 * value + 2, 2) / 2;
  if (name === "EASEINSINE") return 1 - Math.cos(value * Math.PI / 2);
  return value;
}

function smoothStep(value: number) { return value * value * (3 - 2 * value); }
