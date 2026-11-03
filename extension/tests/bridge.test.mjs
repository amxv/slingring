import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {JSDOM} from 'jsdom';

const source=readFileSync(new URL('../bridge.js',import.meta.url),'utf8');
const target='https://chatgpt.com/c/existing-conversation';
const bootstrap='about:blank#slingring-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';
async function run({task={},resultUrl=target,senderUrl=target,prepareError=null,tabUrl=bootstrap,initialBlank=false}={}) {
  const page=new JSDOM('<body></body>',{url:'chrome-extension://test-extension/bridge.html#token=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa&job=test-job',runScripts:'outside-only'});
  const w=page.window,events=[],navigations=[],contentRequests=[],removed=[],order=[];
  let nativeListener,runtimeListener,resolve,updatedListener,tabCommitted=!initialBlank;
  const finished=new Promise(done=>{resolve=done;});
  const post=value=>w.queueMicrotask(()=>nativeListener(value));
  const native={onMessage:{addListener:fn=>{nativeListener=fn;}},onDisconnect:{addListener:()=>{}},postMessage:message=>{
    events.push(message);
    if(message.type==='hello')post({type:'task',job:'test-job',prompt:'test message',effort:null,image_count:0,chatgpt_url:target,tab_url:bootstrap,...task});
    if(message.type==='bind_tab'){assert.equal(tabCommitted,true);post({type:'tab_bound',ok:true});}
    if(message.type==='upload')post({type:'uploaded',ok:true});
    if(message.type==='will_submit')post({type:'continue'});
    if(message.type==='started')post({type:'ack'});
    if(['failed','released'].includes(message.type))resolve(message);
  }};
  const sender={id:'test-extension',tab:{id:5},frameId:0,url:senderUrl};
  w.chrome={runtime:{id:'test-extension',connectNative:()=>native,onMessage:{addListener:fn=>{runtimeListener=fn;}}},tabs:{
    onUpdated:{addListener:fn=>{updatedListener=fn;},removeListener:()=>{updatedListener=null;}},
    create:async data=>{navigations.push(data.url);return {id:5};},
    get:async id=>{
      assert.equal(id,5);
      if(!tabCommitted){w.setTimeout(()=>{tabCommitted=true;updatedListener?.(id,{url:tabUrl},{id,url:tabUrl});},10);return {id,url:'about:blank'};}
      return {id,url:tabUrl};
    },
    update:async (id,data)=>{
      assert.equal(id,5);navigations.push(data.url);order.push(`navigate:${data.url}`);
      w.queueMicrotask(()=>runtimeListener({kind:'slingring',event:'ready'},{...sender,url:data.url},()=>{}));
    },
    sendMessage:async (id,message)=>{
      assert.equal(id,5);contentRequests.push(message);order.push(message.action);
      if(message.action==='prepare')return prepareError ? {ok:false,code:prepareError,submitted:false} :
        {ok:true,effort:task.effort??null,upload_selector:'input[type="file"][multiple]'};
      const ack=await new Promise(done=>runtimeListener({kind:'slingring',event:'will_submit',id:'test-job'},sender,done));
      if(!ack?.ok)return {ok:false,submitted:false,code:'submission_not_authorized'};
      return {ok:true,url:resultUrl,evidence:'thinking',effort:task.effort??null,submitted:true};
    },
    remove:async id=>{removed.push(id);},getCurrent:async()=>({id:99})
  }};
  w.eval(source);
  const terminal=await finished;
  await Promise.resolve();
  const result={terminal,events,navigations,contentRequests,removed,order};
  w.close();return result;
}

test('bridge navigates straight to an exact follow-up with no home visit or mode bootstrap',async()=>{
  const r=await run();
  assert.deepEqual(r.navigations,[bootstrap,target]);
  assert.equal(r.events.filter(e=>e.type==='bind_tab').length,1);
  assert.deepEqual(r.order,[`navigate:${target}`,'prepare','submit']);
  assert.equal(r.contentRequests.find(m=>m.action==='prepare').chatgpt_url,target);
  assert.equal(r.contentRequests.some(m=>m.action==='ensure_chat'),false);
  assert.equal(r.navigations.includes('https://chatgpt.com/'),false);
  assert.equal(r.contentRequests.filter(m=>m.action==='submit').length,1);
  assert.equal(r.terminal.type,'released');assert(r.removed.includes(5));
  assert.equal(r.events.find(e=>e.type==='started').url,target);
});

test('new_task still navigates home and allows its newly created conversation URL',async()=>{
  const r=await run({task:{chatgpt_url:null},senderUrl:'https://chatgpt.com/'});
  assert.deepEqual(r.navigations,[bootstrap,'https://chatgpt.com/']);
  assert.equal(r.contentRequests[0].chatgpt_url,null);assert.equal(r.terminal.type,'released');
  assert.equal(r.contentRequests.some(m=>m.action==='ensure_chat'),false);
});

test('local follow-up images use the existing native upload handshake before submission',async()=>{
  const r=await run({task:{image_count:2}});
  assert.deepEqual(r.navigations,[bootstrap,target]);
  assert.equal(r.contentRequests.find(m=>m.action==='prepare').image_count,2);
  assert.equal(r.events.filter(e=>e.type==='upload').length,1);
  assert.equal(r.events.find(e=>e.type==='upload').selector,'input[type="file"][multiple]');
  assert.equal(r.events.filter(e=>e.type==='will_submit').length,1);
  assert.equal(r.terminal.type,'released');
});

test('an inaccessible/unready target aborts without mode switching or authorizing Send',async()=>{
  const r=await run({prepareError:'conversation_not_ready'});
  assert.deepEqual(r.navigations,[bootstrap,target]);
  assert.deepEqual(r.contentRequests.map(m=>m.action),['prepare']);
  assert.equal(r.terminal.code,'conversation_not_ready');assert.equal(r.terminal.submitted,false);
  assert.equal(r.events.some(e=>e.type==='will_submit'),false);
});

test('an expired job deadline prevents bootstrap navigation and any content action',async()=>{
  const r=await run({task:{deadline_ms:Date.now()-1}});
  assert.equal(r.terminal.code,'deadline_before_submission');assert.equal(r.terminal.submitted,false);
  assert.equal(r.navigations.length,0);assert.equal(r.contentRequests.length,0);
});

test('bridge rejects malformed targets or unsupported efforts before creating a browser tab',async()=>{
  for(const task of [{chatgpt_url:target+'\n'},{chatgpt_url:target+'?x=1'},{chatgpt_url:'https://chatgpt.com/c/'+ 'a'.repeat(129)},
    {chatgpt_url:false},{chatgpt_url:''},{effort:'Ultra'}]) {
    const r=await run({task});assert.equal(r.terminal.type,'failed');assert.equal(r.terminal.code,'invalid_followup');
    assert.equal(r.terminal.submitted,false);assert.equal(r.navigations.length,0);
  }
});

test('follow-ups can select an explicit reasoning effort without visiting the homepage',async()=>{
  const r=await run({task:{effort:'Extra High'}});
  assert.deepEqual(r.navigations,[bootstrap,target]);
  assert.equal(r.contentRequests[0].effort,'Extra High');
  assert.equal(r.events.find(e=>e.type==='started').effort,'Extra High');
  assert.equal(r.terminal.type,'released');
});

test('bridge rejects malformed or excessive local image counts before opening a tab',async()=>{
  for(const image_count of [-1,17,1.5,'1',null]) {
    const r=await run({task:{image_count}});
    assert.equal(r.terminal.code,'invalid_image_count');assert.equal(r.navigations.length,0);
  }
});

test('bridge rejects an ACK request from the wrong conversation before native submission intent',async()=>{
  const r=await run({senderUrl:'https://chatgpt.com/c/other'});
  assert.equal(r.terminal.code,'submission_not_authorized');assert.equal(r.terminal.submitted,false);
  assert.equal(r.events.filter(e=>e.type==='will_submit').length,0);
});

test('bridge rejects wrong or malformed result URLs after submission without retrying',async()=>{
  for(const resultUrl of ['https://chatgpt.com/c/other',target+'\n',target+'#changed']) {
    const r=await run({resultUrl});assert.equal(r.terminal.code,'invalid_result');assert.equal(r.terminal.submitted,true);
    assert.equal(r.events.filter(e=>e.type==='started').length,0);
    assert.equal(r.contentRequests.filter(m=>m.action==='submit').length,1);
  }
});

test('native tab binding is required before navigation and rejects a changed bootstrap tab',async()=>{
  const invalid=await run({task:{tab_url:'about:blank'}});
  assert.equal(invalid.terminal.code,'invalid_tab_bootstrap');assert.equal(invalid.navigations.length,0);
  const moved=await run({tabUrl:'https://chatgpt.com/c/another'});
  assert.equal(moved.terminal.code,'tab_changed_before_navigation');
  assert.deepEqual(moved.navigations,[bootstrap]);assert.equal(moved.contentRequests.length,0);
});

test('native tab binding waits for the nonce URL to commit before querying CDP',async()=>{
  const result=await run({initialBlank:true});
  assert.equal(result.terminal.type,'released');assert.equal(result.events.filter(e=>e.type==='bind_tab').length,1);
});
