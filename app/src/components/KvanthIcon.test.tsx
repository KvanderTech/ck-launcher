import { render } from '@testing-library/react';
import { expect, it } from 'vitest';
import { KvanthIcon, type KvanthIconName } from './KvanthIcon';

it('loads every approved icon as a decorative asset', () => {
  const names = ['home','library','catalog','skins','settings','account','add','play','stop','console','download','import','back','forward','dropdown','expand','minimize','maximize','close','confirm','refresh','external','copy','rename','favorite','folder','file','image','mod','resources','shaders','custom-pack','delete','logout','add-account','restore','telegram','discord','github','modrinth','curseforge','search'] as const;
  const { container } = render(<>{names.map(name => <KvanthIcon key={name} name={name as KvanthIconName | 'search'} />)}</>);
  const icons = container.querySelectorAll('img');
  expect(icons).toHaveLength(42);
  icons.forEach(icon => {
    expect(icon.getAttribute('src')).toBeTruthy();
    expect(icon.getAttribute('aria-hidden')).toBe('true');
    expect(icon.getAttribute('alt')).toBe('');
  });
});
