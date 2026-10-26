// Local visual fixture; no live model requests or stored credentials.
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '../crates/deskpet-app/frontend');
const bootstrap = `<script>
Object.defineProperty(navigator, 'userAgent', {value:'Android DeskPet Preview'});
const previewListeners = {};
window.__TAURI__ = {
  event:{listen:(name,callback)=>{(previewListeners[name] ||= []).push(callback);return Promise.resolve(()=>{});}},
  core:{invoke:async(name)=>{
    if(name==='get_settings') return {pet:{name:'基尔兽',personality:'友好',speaking_style:'轻松',background:''},apiBase:'https://example.com/v1',model:'preview'};
    if(name==='trigger_agent') {
      const payload = {title:'今天，也有值得开心的小事',script:'抬头看看窗外，给自己留一点休息时间。\\n我会在这里，陪你度过今天。'.repeat(6),hot_id:'preview',mood_hint:'happy'};
      (previewListeners['bubble:show'] || []).forEach(callback=>callback({payload}));
    }
    return null;
  }}
};
</script>`;
http.createServer((req,res)=>{
  const url = new URL(req.url, 'http://localhost');
  const file = path.resolve(root, '.' + (url.pathname === '/' ? '/index.html' : decodeURIComponent(url.pathname)));
  if(!file.startsWith(root + path.sep)){res.writeHead(403);return res.end();}
  fs.readFile(file,(error,bytes)=>{
    if(error){res.writeHead(404);return res.end();}
    res.setHeader('Content-Type', {'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.png':'image/png'}[path.extname(file)] || 'application/octet-stream');
    if(path.extname(file)==='.html') bytes=bytes.toString('utf8').replace('<head>', '<head>'+bootstrap);
    res.end(bytes);
  });
}).listen(4173,'127.0.0.1',()=>console.log('Companion preview: http://127.0.0.1:4173'));
