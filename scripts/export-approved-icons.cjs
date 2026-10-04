const fs = require('node:fs');
const path = require('node:path');
const sharp = require('sharp');
const root = path.resolve(__dirname, '..');
const source = process.argv[2];
if (!source) throw new Error('Pass the approved white contact-sheet directory.');
const out = path.join(root, 'app/src/assets/icons');
fs.mkdirSync(out, { recursive: true });
const groups = [
  ['01-navigation-actions.png', ['home','library','catalog','skins','settings','account','add','play','stop','console','download','import']],
  ['02-controls.png', ['back','forward','dropdown','expand','minimize','maximize','close','confirm','refresh','external','copy','rename']],
  ['03-files-content.png', ['favorite','folder','file','image','mod','resources','shaders','custom-pack','delete','logout','add-account','restore']],
];
async function exportIcon(file, name, box) {
  const { data, info } = await sharp(path.join(source,file)).extract(box).ensureAlpha().raw().toBuffer({resolveWithObject:true});
  let minX=info.width, minY=info.height, maxX=0, maxY=0;
  for (let y=0;y<info.height;y++) for(let x=0;x<info.width;x++) {
    const i=(y*info.width+x)*4;
    const light=Math.max(data[i],data[i+1],data[i+2]);
    // The approved glyphs are white/gray, the sheet background is dark navy.
    const alpha=Math.round(Math.max(0,Math.min(1,(light-65)/65))*255);
    data[i+3]=alpha;
    if(alpha>128){minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y);}
    if(alpha>0 && alpha<255) data[i]=data[i+1]=data[i+2]=235;
  }
  if(minX>maxX) throw new Error(`Empty icon: ${name}`);
  await sharp(data,{raw:{width:info.width,height:info.height,channels:4}})
    .extract({left:Math.max(0,minX-2),top:Math.max(0,minY-2),width:Math.min(info.width-minX+2,maxX-minX+5),height:Math.min(info.height-minY+2,maxY-minY+5)})
    .resize(120,120,{fit:'contain',background:'#00000000'})
    .extend({top:4,bottom:4,left:4,right:4,background:'#00000000'})
    .png().toFile(path.join(out,`${name}.png`));
}
(async()=>{
  for(const [file,names] of groups) for(let i=0;i<names.length;i++) {
    const row=Math.floor(i/4), col=i%4;
    await exportIcon(file,names[i],{left:col*362+8,top:[35,385,720][row],width:346,height:(file === '02-controls.png' ? [285,245,250] : [285,280,265])[row]});
  }
  for(const [name,box] of [
    ['telegram',{left:35,top:30,width:470,height:430}],
    ['discord',{left:535,top:30,width:470,height:430}],
    ['github',{left:1045,top:30,width:470,height:430}],
    ['modrinth',{left:280,top:535,width:450,height:390}],
    ['curseforge',{left:795,top:535,width:520,height:390}],
  ]) await exportIcon('04-services.png',name,box);
  console.log('Exported 41 approved white icons as transparent 128px PNGs.');
})();
