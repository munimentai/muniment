import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve, extname} from 'node:path';
const root=process.cwd();
createServer(async(req,res)=>{
 try {
  if(req.url==='/'){res.writeHead(302,{Location:'/test/probe/local-mode.html'});res.end();return;}
  const path=decodeURIComponent(new URL(req.url,'http://localhost').pathname);
  const relative=path.startsWith('/assets/')?'/dist'+path:path==='/'?'/test/probe/local-mode.html':path;
  const file=resolve(root,'.'+relative);
  if(!file.startsWith(root+'/')) {res.writeHead(403);res.end();return;}
  const body=await readFile(file);
  const type={'.js':'text/javascript','.css':'text/css','.html':'text/html','.svg':'image/svg+xml','.json':'application/json','.woff2':'font/woff2','.png':'image/png'}[extname(file)]||'application/octet-stream';
  res.writeHead(200,{'Content-Type':type,'Cache-Control':'no-store'});res.end(body);
 }catch{res.writeHead(404);res.end('Not found');}
}).listen(4345,'127.0.0.1',()=>console.log('Desktop preview: http://127.0.0.1:4345'));
