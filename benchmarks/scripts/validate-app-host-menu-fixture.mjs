// Source-only deterministic fixture checks. Not native menu, clipboard, or App acceptance.
import {readFileSync} from 'node:fs';
import assert from 'node:assert/strict';
import vm from 'node:vm';
const filename=new URL('../fixtures/app-host-context-menu/index.html',import.meta.url);
const html=readFileSync(filename,'utf8').replaceAll('__NONCE__','12-34');
const script=html.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(script);
new vm.Script(script,{filename:filename.pathname});
const controls=Object.fromEntries(['copy','paste','note','split'].map(id=>[id,{id,value:id==='copy'?'copy-12-34':'',listeners:{},addEventListener(type,handler){this.listeners[type]=handler;}}]));
const attributes=new Map();const listeners={};const calls=[];
const document={readyState:'complete',documentElement:{setAttribute(key,value){attributes.set(key,value);},getAttribute(key){return attributes.get(key)??null;}},getElementById(id){return controls[id];},addEventListener(type,handler){listeners[type]=handler;}};
const context=vm.createContext({document,window:{},location:{href:'http://127.0.0.1:1/source?nonce=12-34'},performance:{timeOrigin:42},URLSearchParams,fetch:async target=>{calls.push(target);const url=new URL(target,'http://127.0.0.1:1');return{text:async()=>`12-34:${url.searchParams.get('kind')}`};}});
vm.runInContext(script,context);
listeners.contextmenu({isTrusted:false,button:2,target:controls.copy});
listeners.contextmenu({isTrusted:true,button:0,target:controls.copy});
listeners.contextmenu({isTrusted:true,button:2,target:controls.copy});
assert.equal(context.window.hostMenuSnapshot().trusted.copy,1);
assert.equal(context.window.hostMenuSnapshot().trusted.note,0);
for(const kind of ['paste','note']){controls[kind].value=`${kind}-12-34`;controls[kind].listeners.input({target:controls[kind],isTrusted:kind==='paste'});}
await new Promise(resolve=>setImmediate(resolve));
const snapshot=context.window.hostMenuSnapshot();
assert.equal(snapshot.copy,'copy-12-34');assert.equal(snapshot.paste,'paste-12-34');assert.equal(snapshot.note,'note-12-34');
assert.equal(snapshot.pasteAck,'12-34');assert.equal(snapshot.noteAck,'12-34');assert.equal(snapshot.effects.paste.length,1);assert.equal(snapshot.effects.note.length,1);assert.equal(snapshot.effects.paste[0].trusted,true);
assert.equal(calls.length,2);
for(const [index,kind] of ['paste','note'].entries()){const url=new URL(calls[index],'http://127.0.0.1:1');assert.equal(url.pathname,'/effect');assert.equal(url.searchParams.get('nonce'),'12-34');assert.equal(url.searchParams.get('kind'),kind);assert.equal(url.searchParams.get('value'),`${kind}-12-34`);}
assert.match(html,/id="split" href="\/destination\?nonce=12-34"/);
console.log('PASS: source fixture syntax, trusted right-click accounting, exact tokens, nonce acknowledgments, and split destination. Native App/clipboard behavior NOT RUN.');
