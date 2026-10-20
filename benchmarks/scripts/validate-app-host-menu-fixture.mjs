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
const context=vm.createContext({document,window:{},location:{href:'http://127.0.0.1:1/source?nonce=12-34'},performance:{timeOrigin:42},URLSearchParams,fetch:async target=>{calls.push(target);const url=new URL(target,'http://127.0.0.1:1');return{status:200,text:async()=>`12-34:${url.searchParams.get('kind')}`};}});
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
for(const kind of ['paste','note']){assert.equal(snapshot.network[kind].started,true);assert.equal(snapshot.network[kind].response_status,200);assert.equal(snapshot.network[kind].exact_body_match,true);assert.equal(snapshot.network[kind].rejected,null);}
for(const [index,kind] of ['paste','note'].entries()){const url=new URL(calls[index],'http://127.0.0.1:1');assert.equal(url.pathname,'/effect');assert.equal(url.searchParams.get('nonce'),'12-34');assert.equal(url.searchParams.get('kind'),kind);assert.equal(url.searchParams.get('value'),`${kind}-12-34`);}
assert.match(html,/id="split" href="\/destination\?nonce=12-34"/);
for(const failure of ['mismatch','rejected','unknown-error']){
 const localControls=Object.fromEntries(['copy','paste','note','split'].map(id=>[id,{id,value:id==='copy'?'copy-12-34':'',listeners:{},addEventListener(type,handler){this.listeners[type]=handler;}}]));
 const localAttributes=new Map();let fetches=0;
 const localDocument={...document,getElementById(id){return localControls[id];},addEventListener(){},documentElement:{setAttribute(k,v){localAttributes.set(k,v);},getAttribute(k){return localAttributes.get(k)??null;}}};
 const failed=vm.createContext({document:localDocument,window:{},location:{href:'http://127.0.0.1:1/source?nonce=12-34'},performance:{timeOrigin:42},URLSearchParams,fetch:async()=>{fetches++;if(failure==='rejected')throw new TypeError('synthetic transport failure');if(failure==='unknown-error')throw {name:'arbitrary-unlogged-value'};return{status:200,text:async()=> 'wrong-body'};}});
 vm.runInContext(script,failed);localControls.note.value='note-12-34';localControls.note.listeners.input({target:localControls.note,isTrusted:true});await new Promise(resolve=>setImmediate(resolve));
 const observed=failed.window.hostMenuSnapshot();assert.equal(fetches,1);assert.equal(observed.noteAck,null);assert.equal(observed.note,'note-12-34');assert.equal(observed.effects.note.length,1);assert.equal(observed.effects.note[0].trusted,true);assert.equal(observed.network.note.started,true);
 assert.equal(observed.network.note.response_status,failure==='mismatch'?200:null);assert.equal(observed.network.note.exact_body_match,failure==='mismatch'?false:null);assert.equal(observed.network.note.rejected,failure==='rejected'?'TypeError':failure==='unknown-error'?'OtherError':null);
}
console.log('PASS: source fixture syntax, trusted accounting, exact acknowledgments and fixed network failure states without retry/fallback. Native App/clipboard/socket behavior NOT RUN.');
