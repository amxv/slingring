import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {JSDOM} from 'jsdom';
const source=readFileSync(new URL('../content.js',import.meta.url),'utf8');

test('submission re-queries a React-replaced send button after the native acknowledgement',async()=>{
  const page=new JSDOM('<main><textarea id="prompt"></textarea><button id="send">Send</button></main>',{url:'https://chatgpt.com/',runScripts:'outside-only'});
  const w=page.window,d=w.document;
  let listener,submitted=0;
  const old=d.querySelector('#send');
  old.addEventListener('click',()=>assert.fail('detached stale button was clicked'));
  w.__slingringDOM={
    ensureChat:async()=>{},setEffort:async()=> 'Medium',fill:value=>{d.querySelector('#prompt').value=value;},
    hasDraft:()=>false,noAttachments:()=>true,followupIdle:()=>true,visible:()=>true,stableComposer:async()=>d.querySelector('#prompt'),
    promptEquals:(input,value)=>input?.value===value,promptState:value=>d.querySelector('#prompt').value===value?'matches':'prompt_changed',
    composer:()=>d.querySelector('#prompt'),text:e=>e.value.trim(),normal:value=>value.trim(),
    uploadSelector:async()=>null,attachmentsReady:()=>true,sendButton:()=>d.querySelector('#send'),
    chatVerified:()=>true,currentEffort:()=> 'Medium',canonicalUrl:()=>submitted?'https://chatgpt.com/c/test':null,
    turnBaseline:()=>({users:[]}),
    startEvidence:()=>submitted?'thinking':null,
    waitFor:async predicate=>{const value=predicate();if(!value)throw Error('predicate_not_ready');return value;}
  };
  w.chrome={runtime:{id:'test-extension',onMessage:{addListener:fn=>{listener=fn;}},sendMessage:async message=>{
    if(message.event==='will_submit'){
      const next=d.createElement('button');next.id='send';next.textContent='Send';
      next.addEventListener('click',()=>{submitted++;});old.replaceWith(next);
      return {ok:true};
    }
    return {};
  }}};
  w.eval(source);
  const sender={id:'test-extension',url:'chrome-extension://test-extension/bridge.html'};
  const request=message=>new Promise(resolve=>listener({kind:'slingring',...message},sender,resolve));
  assert.equal((await request({action:'prepare',id:'task',prompt:'test',effort:'Medium',image_count:0})).ok,true);
  const result=await request({action:'submit',id:'task'});
  assert.equal(result.ok,true);assert.equal(result.evidence,'thinking');assert.equal(submitted,1);
  page.window.close();
});
