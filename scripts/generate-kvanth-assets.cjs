// Deterministic packaging of the approved Kvanth raster artwork.
const fs = require('node:fs');
const path = require('node:path');
const sharp = require('sharp');
const root = path.resolve(__dirname, '..');
const source = process.argv[2];
if (!source) throw new Error('Pass the approved Kvanth artwork directory.');
const icons = path.join(root, 'app/src-tauri/icons/kvanth');
const installer = path.join(root, 'app/src-tauri/installer/kvanth');
fs.mkdirSync(icons, { recursive: true });
fs.mkdirSync(installer, { recursive: true });
async function bmp(image, dest) {
  const { data, info } = await image.removeAlpha().raw().toBuffer({ resolveWithObject: true });
  const stride = Math.ceil(info.width * 3 / 4) * 4;
  const out = Buffer.alloc(54 + stride * info.height);
  out.write('BM'); out.writeUInt32LE(out.length, 2); out.writeUInt32LE(54, 10);
  out.writeUInt32LE(40, 14); out.writeInt32LE(info.width, 18); out.writeInt32LE(info.height, 22);
  out.writeUInt16LE(1, 26); out.writeUInt16LE(24, 28); out.writeUInt32LE(stride * info.height, 34);
  for (let y = 0; y < info.height; y++) for (let x = 0; x < info.width; x++) {
    const src = (y * info.width + x) * 3, dst = 54 + (info.height - 1 - y) * stride + x * 3;
    out[dst] = data[src + 2]; out[dst + 1] = data[src + 1]; out[dst + 2] = data[src];
  }
  fs.writeFileSync(dest, out);
}
(async () => {
  const mark = path.join(source, 'kvanth-icon.png');
  const word = path.join(source, 'kvanth-wordmark-white-stacked.png');
  fs.copyFileSync(word, path.join(root, 'app/src/assets/kvanth-wordmark.png'));
  const square = await sharp(mark).resize(460, 460, { fit: 'contain', background: '#00000000' }).extend({ top:26,bottom:26,left:26,right:26,background:'#00000000' }).png().toBuffer();
  await sharp(square).toFile(path.join(icons, 'icon.png'));
  fs.copyFileSync(path.join(icons,'icon.png'),path.join(root,'app/src/assets/kvanth-icon.png'));
  const sizes = [16, 24, 32, 48, 64, 128, 256];
  const images = await Promise.all(sizes.map(size => sharp(square).resize(size,size).png().toBuffer()));
  const header = Buffer.alloc(6 + sizes.length * 16); header.writeUInt16LE(1,2); header.writeUInt16LE(sizes.length,4);
  let offset = header.length;
  sizes.forEach((size,i) => { const p=6+i*16; header[p]=size===256?0:size;header[p+1]=header[p];header.writeUInt16LE(1,p+4);header.writeUInt16LE(32,p+6);header.writeUInt32LE(images[i].length,p+8);header.writeUInt32LE(offset,p+12);offset+=images[i].length; });
  fs.writeFileSync(path.join(icons,'icon.ico'),Buffer.concat([header,...images]));
  for (const [size,name] of [[32,'32x32.png'],[128,'128x128.png'],[256,'128x128@2x.png']]) await sharp(square).resize(size,size).toFile(path.join(icons,name));
  fs.copyFileSync(path.join(icons,'32x32.png'),path.join(root,'app/public/kvanth-icon.png'));
  const sidebarWord=await sharp(word).resize({width:140}).png().toBuffer();
  const sidebarMark=await sharp(square).resize(124,124).png().toBuffer();
  await bmp(sharp({create:{width:164,height:314,channels:3,background:'#071426'}}).composite([{input:sidebarMark,left:20,top:35},{input:sidebarWord,left:12,top:182}]),path.join(installer,'sidebar.bmp'));
  const headerWord=await sharp(word).resize({width:134}).png().toBuffer();
  await bmp(sharp({create:{width:150,height:57,channels:3,background:'#071426'}}).composite([{input:headerWord,left:8,top:3}]),path.join(installer,'header.bmp'));
  console.log('Kvanth icons, wordmark and installer artwork exported.');
})();
