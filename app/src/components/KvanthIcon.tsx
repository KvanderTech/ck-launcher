const assets = import.meta.glob('../assets/icons/*.{png,svg}', { eager: true, query: '?url', import: 'default' }) as Record<string, string>;
const blueAssets = import.meta.glob('../assets/icons/blue/*.png', { eager: true, query: '?url', import: 'default' }) as Record<string, string>;

export type KvanthIconName = 'home' | 'library' | 'catalog' | 'skins' | 'settings' | 'account' | 'add' | 'play' | 'stop' | 'console' | 'download' | 'import' | 'back' | 'forward' | 'dropdown' | 'expand' | 'minimize' | 'maximize' | 'close' | 'confirm' | 'refresh' | 'external' | 'copy' | 'rename' | 'favorite' | 'folder' | 'file' | 'image' | 'mod' | 'resources' | 'shaders' | 'custom-pack' | 'delete' | 'logout' | 'add-account' | 'restore' | 'telegram' | 'discord' | 'github' | 'modrinth' | 'curseforge';

export function KvanthIcon({ name, size = 20, className = '', tone = 'white' }: { name: KvanthIconName | 'search'; size?: number; className?: string; tone?: 'white' | 'blue' }) {
  const src = tone === 'blue' ? blueAssets[`../assets/icons/blue/${name}.png`] : assets[`../assets/icons/${name}.${name === 'search' ? 'svg' : 'png'}`];
  return <img alt="" aria-hidden="true" draggable={false} className={`kvanth-ui-icon ${className}`} data-icon={name} data-tone={tone} src={src} style={{ width: size, height: size }} />;
}
