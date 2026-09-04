import { useEffect } from "react";

import buildSwitchUrl from "../assets/sounds/build-switch.wav";
import clickUrl from "../assets/sounds/click.wav";
import gameExitUrl from "../assets/sounds/game-exit.wav";
import gameReadyUrl from "../assets/sounds/game-ready.wav";
import installCompleteUrl from "../assets/sounds/install-complete.wav";
import launchUrl from "../assets/sounds/launch.wav";
import skinSelectUrl from "../assets/sounds/skin-select.wav";

export type SoundEffect = "click" | "install-complete" | "game-ready" | "game-exit" | "launch" | "build-switch" | "skin-select";

const sources: Record<SoundEffect, string> = {
  "click": clickUrl,
  "install-complete": installCompleteUrl,
  "game-ready": gameReadyUrl,
  "game-exit": gameExitUrl,
  "launch": launchUrl,
  "build-switch": buildSwitchUrl,
  "skin-select": skinSelectUrl,
};
const audio = new Map<SoundEffect, HTMLAudioElement>();

export function playSound(effect: SoundEffect) {
  let base = audio.get(effect);
  if (!base) {
    base = new Audio(sources[effect]);
    base.preload = "auto";
    base.volume = effect === "click" ? 0.42 : 0.55;
    audio.set(effect, base);
  }
  const player = base.paused ? base : base.cloneNode(true) as HTMLAudioElement;
  player.currentTime = 0;
  try {
    const playback = player.play();
    if (playback) void playback.catch(() => undefined);
  } catch {
    // A sound must never block its launcher action.
  }
}

function isSoundEffect(value: string): value is SoundEffect {
  return Object.prototype.hasOwnProperty.call(sources, value);
}

export function SoundEffects() {
  useEffect(() => {
    function handleClick(event: MouseEvent) {
      if (!(event.target instanceof Element)) return;
      const control = event.target.closest<HTMLElement>("button, a, [role='button']");
      if (!control || control.matches(":disabled") || control.getAttribute("aria-disabled") === "true") return;
      const requested = control.dataset.sound;
      if (requested === "none") return;
      playSound(requested && isSoundEffect(requested) ? requested : "click");
    }
    document.addEventListener("click", handleClick);
    return () => document.removeEventListener("click", handleClick);
  }, []);
  return null;
}
