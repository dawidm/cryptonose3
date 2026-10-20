import { convertFileSrc } from "@tauri-apps/api/core";
import { useStore } from "./store";

const cache = new Map<string, HTMLAudioElement>();

function audio(src: string): HTMLAudioElement {
  let a = cache.get(src);
  if (!a) {
    a = new Audio(src);
    cache.set(src, a);
  }
  return a;
}

export function soundSrc(rising: boolean): string {
  const g = useStore.getState().settings?.general;
  const custom = rising ? g?.soundRising : g?.soundFalling;
  return custom ? convertFileSrc(custom) : rising ? "/sounds/rising.wav" : "/sounds/falling.wav";
}

export function playAlertSound(rising: boolean) {
  const a = audio(soundSrc(rising));
  a.currentTime = 0;
  a.play().catch((e) => console.warn("sound failed", e));
}
