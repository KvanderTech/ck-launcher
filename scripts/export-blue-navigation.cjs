const path = require('node:path');
const fs = require('node:fs');
const sharp = require('sharp');
const source = process.argv[2];
const out = path.resolve(__dirname, '../app/src/assets/icons/blue');
fs.mkdirSync(out, { recursive: true });
(async () => {
  for (const [name, col, top, height] of [['home',0,35,285],['library',1,35,285],['catalog',2,35,285],['skins',3,35,285],['settings',0,385,280],['account',1,385,280],['add',2,385,280]]) {
    const { data, info } = await sharp(source).extract({left:col*362+8,top,width:346,height}).ensureAlpha().raw().toBuffer({resolveWithObject:true});
    let x0=info.width,y0=info.height,x1=0,y1=0;
    for(let y=0;y<info.height;y++) for(let x=0;x<info.width;x++) {
      const i=(y*info.width+x)*4;
      const strength=Math.max(data[i+1],data[i+2])-data[i];
      data[i+3]=Math.round(Math.max(0,Math.min(1,(strength-35)/65))*255);
      if(data[i+3]>128){x0=Math.min(x0,x);y0=Math.min(y0,y);x1=Math.max(x1,x);y1=Math.max(y1,y);}
    }
    await sharp(data,{raw:{width:info.width,height:info.height,channels:4}}).extract({left:x0,top:y0,width:x1-x0+1,height:y1-y0+1}).resize(120,120,{fit:'contain',background:'#00000000'}).extend({top:4,bottom:4,left:4,right:4,background:'#00000000'}).png().toFile(path.join(out,name+'.png'));
  }
})();
