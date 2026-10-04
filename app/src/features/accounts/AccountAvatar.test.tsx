import { cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { AccountAvatar, drawSkinHead } from './AccountAvatar';

afterEach(cleanup);
const account = { id:'one', minecraftName:'Alex', minecraftUuid:'uuid', isActive:true, headUrl:'https://example.test/cached.png' };
it('crops the face and hat layer with crisp pixels, including HD skins', () => {
  for (const scale of [1,2,4]) {
    const context = { clearRect:vi.fn(), drawImage:vi.fn(), imageSmoothingEnabled:true };
    const skin = { naturalWidth:64*scale, naturalHeight:64*scale } as HTMLImageElement;
    expect(drawSkinHead(context as unknown as CanvasRenderingContext2D, skin)).toBe(true);
    expect(context.drawImage).toHaveBeenNthCalledWith(1,skin,8*scale,8*scale,8*scale,8*scale,0,0,8,8);
    expect(context.drawImage).toHaveBeenNthCalledWith(2,skin,40*scale,8*scale,8*scale,8*scale,0,0,8,8);
    expect(context.imageSmoothingEnabled).toBe(false);
  }
});
it('rejects an avatar response where a full skin texture is expected', () => {
  const context = { clearRect:vi.fn(), drawImage:vi.fn() };
  expect(drawSkinHead(context as unknown as CanvasRenderingContext2D, {naturalWidth:40,naturalHeight:40} as HTMLImageElement)).toBe(false);
  expect(context.drawImage).not.toHaveBeenCalled();
});
it('prefers the profile skin over the cached third-party head', () => {
  const {container,rerender} = render(<AccountAvatar account={account} skinUrl="https://textures.minecraft.net/texture/first" />);
  expect(container.querySelector('img[src="https://example.test/cached.png"]')).toBeNull();
  expect(container.querySelector('canvas')?.getAttribute('data-skin-url')).toContain('/first');
  rerender(<AccountAvatar account={account} skinUrl="https://textures.minecraft.net/texture/second" />);
  expect(container.querySelector('canvas')?.getAttribute('data-skin-url')).toContain('/second');
});
it('shows a neutral fallback when the legacy head cannot load', () => {
  const {container} = render(<AccountAvatar account={account} />);
  fireEvent.error(container.querySelector('img')!);
  expect(container.querySelector('[data-icon="account"]')).toBeTruthy();
});
