import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {JSDOM} from 'jsdom';
const source=readFileSync(new URL('../dom.js',import.meta.url),'utf8');
const url='https://chatgpt.com/c/pause-fixture';
const user='<div data-message-author-role="user">Do not read</div>';
const footer='<button aria-label="Copy response"></button><button aria-label="Good response"></button><button aria-label="Bad response"></button>';
const turn=body=>`<article data-turn="assistant">${body}</article>`;
const composer='<form><textarea id="prompt-textarea"></textarea><button aria-label="Start voice mode"></button></form>';
function page(body){
 const p=new JSDOM(`<main>${body}${composer}</main>`,{url,runScripts:'outside-only'}),w=p.window;
 w.HTMLElement.prototype.getClientRects=function(){return this.closest('[hidden]')?[]:[{}];};
 w.eval(source);
 // The pause path must not read model text, even as a presence heuristic.
 Object.defineProperty(w.Node.prototype,'textContent',{get(){throw Error('text_read_forbidden');},set(){throw Error('text_write_forbidden');}});
 return {w,d:w.document,api:w.__slingringDOM,close:()=>w.close()};
}
test('pause needs latest assistant completion controls, not voice or historical footer',()=>{
 for(const body of [user,user+turn(''),user+turn(footer)+user,user+turn(footer)+user+turn(''),user+turn(`<div hidden>${footer}</div>`),user+turn(`<div class="markdown">${footer}</div>`)]){
  const p=page(body);assert.equal(p.api.pauseObservation().complete,false);p.close();
 }
 const p=page(user+turn(footer));assert.equal(p.api.pauseObservation().complete,true);assert.equal(p.api.pauseObservation().active,false);p.close();
});
test('busy indicators defeat completion and disabled or ambiguous Stops are not clicked',()=>{
 for(const signal of ['<div aria-busy="true"></div>','<div class="pulsing-dot"></div>','<div data-is-streaming="true"></div>']){
  const p=page(user+turn(footer+signal));assert.equal(p.api.pauseObservation().active,true);p.close();
 }
 const p=page(user+turn(footer));p.d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Stop" disabled></button>');
 assert.equal(p.api.pauseObservation().active,true);assert.equal(p.api.pauseObservation().stop,null);
 p.d.querySelector('[disabled]').disabled=false;p.d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Stop"></button>');
 assert.equal(p.api.pauseObservation().stop,null);p.close();
});
test('stable completed latest turn returns already idle without touching text',async()=>{
 const p=page(user+turn(footer));const result=await p.api.pauseTask(url,Date.now()+5000);
 assert.equal(result.status,'already_idle');p.close();
});
test('one Stop click requires repeated completion verification',async()=>{
 const p=page(user+turn(''));p.d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Stop" id="stop"></button>');
 let clicks=0;p.d.querySelector('#stop').onclick=()=>{clicks++;p.d.querySelector('#stop').remove();p.d.querySelector('article').innerHTML=footer;};
 const result=await p.api.pauseTask(url,Date.now()+5000);assert.equal(result.status,'stopped');assert.equal(clicks,1);p.close();
});
test('disappearing Stop without footer is uncertainty, never success',async()=>{
 const p=page(user+turn(''));p.d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Stop" id="stop"></button>');
 let clicks=0;p.d.querySelector('#stop').onclick=()=>{clicks++;p.d.querySelector('#stop').remove();};
 await assert.rejects(p.api.pauseTask(url,Date.now()+600),/pause_unverified/);assert.equal(clicks,1);p.close();
});
test('voice without Stop/footer times out; navigation is never acted on',async()=>{
 const p=page(user+turn(''));await assert.rejects(p.api.pauseTask(url,Date.now()+600),/pause_stop_unavailable/);
 await assert.rejects(p.api.pauseTask('https://chatgpt.com/c/other',Date.now()+1000),/conversation_changed/);p.close();
});
test('rehydrated footer must restart stable observation',async()=>{
 const p=page(user+turn(footer));p.w.setTimeout(()=>{p.d.querySelector('article').outerHTML=turn(footer);},2000);
 await assert.rejects(p.api.pauseTask(url,Date.now()+3900),/pause_stop_unavailable/);p.close();
});
test('a newer user turn after Stop cannot verify the earlier cancellation',async()=>{
 const p=page(user+turn(''));p.d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Stop" id="stop"></button>');
 p.d.querySelector('#stop').onclick=()=>{
  p.d.querySelector('#stop').remove();p.d.querySelector('form').insertAdjacentHTML('beforebegin',user+turn(footer));
 };
 await assert.rejects(p.api.pauseTask(url,Date.now()+2000),/pause_unverified/);p.close();
});
test('quoted Stop controls inside an answer are never clicked',()=>{
 const p=page(user+turn('<button aria-label="Stop"></button>'));
 assert.equal(p.api.pauseObservation().stop,null);p.close();
});
const thinkingStatus='<button id="thinking"><span id="status-label">Stopped thinking</span><svg></svg></button>';
const pair='<div><div><button data-dictation-button aria-label="Dictate"></button></div><div><button aria-label="Start Voice"></button></div></div>';
function stoppedPage(body=`<div data-talvt-turn-state>${user}${thinkingStatus}</div>`,withPair=true){
 const p=page(body);p.d.querySelector('form button').remove();
 if(withPair)p.d.querySelector('form').insertAdjacentHTML('beforeend',pair);
 for(const span of p.d.querySelectorAll('#status-label'))Object.defineProperty(span,'textContent',{get(){return this.firstChild?.nodeValue || '';}});
 return p;
}
test('busy current-turn or main ancestors defeat stopped-thinking confirmation',()=>{
 for(const selector of ['[data-talvt-turn-state]','main']) {
  const p=stoppedPage();p.d.querySelector(selector).setAttribute('aria-busy','true');
  assert(p.api.pauseObservation().stopped);assert.equal(p.api.pauseObservation().active,true);p.close();
 }
});
test('shortcut-labelled Stop prevents stale footer idle detection',()=>{
 const p=page(user+turn(footer));p.d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Stop generating (Esc)"></button>');
 assert.equal(p.api.pauseObservation().active,true);assert(p.api.pauseObservation().stop);p.close();
});
test('a direct-child empty latest assistant never borrows old footer controls from main',()=>{
 const p=page(user+turn(footer)+user+'<div data-conversation-role="assistant"></div>');
 assert.equal(p.api.pauseObservation().complete,false);p.close();
});
test('early stopped-thinking status plus adjacent Dictate/Voice verifies without response reads',async()=>{
 const p=stoppedPage();assert(p.api.pauseObservation().stopped);
 const result=await p.api.pauseTask(url,Date.now()+5000);assert.equal(result.evidence,'stopped_thinking');assert.equal(result.status,'already_idle');p.close();
});
test('stale, response-body, unscoped, or non-adjacent status combinations are not evidence',()=>{
 const bodies=[`<div data-talvt-turn-state>${user}${thinkingStatus}</div>${user}`,
  `<div data-talvt-turn-state>${user}<div class="markdown">${thinkingStatus}</div></div>`,
  user+thinkingStatus,`<div data-talvt-turn-state>${user}${thinkingStatus}${user}</div>`];
 for(const body of bodies){const p=stoppedPage(body);assert.equal(p.api.pauseObservation().stopped,null);p.close();}
 const p=stoppedPage(undefined,false);assert.equal(p.api.pauseObservation().stopped,null);
 p.d.querySelector('form').insertAdjacentHTML('beforeend','<div><button data-dictation-button aria-label="Dictate"></button></div><div><button aria-label="Start Voice"></button></div>');
 assert.equal(p.api.pauseObservation().stopped,null);p.close();
});
test('thinking UI changes to stopped only after one Stop click and stable paired controls',async()=>{
 const p=stoppedPage();p.d.querySelector('#status-label').firstChild.nodeValue='Thinking';
 p.d.querySelector('[aria-label="Start Voice"]').setAttribute('aria-label','Stop');
 assert.equal(p.api.pauseObservation().stopped,null);
 p.d.querySelector('[aria-label="Stop"]').onclick=e=>{
  e.currentTarget.setAttribute('aria-label','Start Voice');p.d.querySelector('#status-label').firstChild.nodeValue='Stopped thinking';
 };
 const r=await p.api.pauseTask(url,Date.now()+5000);assert.equal(r.status,'stopped');assert.equal(r.evidence,'stopped_thinking');p.close();
});
