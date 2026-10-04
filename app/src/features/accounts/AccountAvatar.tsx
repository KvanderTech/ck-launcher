import { useEffect, useRef, useState } from 'react';
import type { AccountSummary } from '../../app/types';
import { KvanthIcon } from '../../components/KvanthIcon';

export function drawSkinHead(context: CanvasRenderingContext2D, skin: HTMLImageElement) {
  const scale = skin.naturalWidth / 64;
  if (!Number.isInteger(scale) || scale < 1 || ![32 * scale, 64 * scale].includes(skin.naturalHeight)) return false;
  context.imageSmoothingEnabled = false;
  context.clearRect(0, 0, 8, 8);
  context.drawImage(skin, 8 * scale, 8 * scale, 8 * scale, 8 * scale, 0, 0, 8, 8);
  context.drawImage(skin, 40 * scale, 8 * scale, 8 * scale, 8 * scale, 0, 0, 8, 8);
  return true;
}

export function AccountAvatar({ account, skinUrl }: { account: AccountSummary; skinUrl?: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [ready, setReady] = useState(false);
  const [headFailed, setHeadFailed] = useState(false);
  useEffect(() => {
    setReady(false);
    setHeadFailed(false);
    if (!skinUrl) return;
    let disposed = false;
    const image = new Image();
    image.onload = () => {
      if (disposed) return;
      const context = canvas.current?.getContext('2d');
      if (context) setReady(drawSkinHead(context, image));
    };
    image.src = skinUrl.replace(/^http:\/\//, 'https://');
    return () => { disposed = true; image.onload = null; image.onerror = null; };
  }, [skinUrl, account.id]);
  // The current profile texture wins over cached third-party avatar services.
  if (!skinUrl && account.headUrl && !headFailed) return <img className="account-avatar" alt="" height={40} width={40} src={account.headUrl} onError={() => setHeadFailed(true)} />;
  return <span className="avatar-fallback account-avatar" aria-hidden="true">
    {skinUrl && <canvas ref={canvas} width={8} height={8} className="account-skin-head" data-skin-url={skinUrl} style={{ display: ready ? 'block' : 'none' }} />}
    {!ready && <KvanthIcon name="account" size={28} />}
  </span>;
}
