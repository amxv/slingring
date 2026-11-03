import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {JSDOM} from 'jsdom';

const source=readFileSync(new URL('../bridge.js',import.meta.url),'utf8');
const target='https://chatgpt.com/c/existing-conversation';
async function run({task={},resultUrl=target,senderUrl=target,bootstrapError=null}={}) {
  const page=new JSDOM('<body></body>',{url:'chrome-extension://test-extension/bridge.html#token=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa&job=test-job',runScripts:'outside-only'});
  const w=page.window,events=[],navigations=[],contentRequests=[],removed=[],order=[];
  let nativeListener,runtimeListener,resolve;
  const finished=new Promise(done=>{resolve=done;});
  const post=value=>w.queueMicrotask(()=>nativeListener(value));
  const native={onMessage:{addListener:fn=>{nativeListener=fn;}},onDisconnect:{addListener:()=>{}},postMessage:message=>{
    events.push(message);
    if(message.type==='hello')post({type:'task',job:'test-job',prompt:'test message',effort:null,image_count:0,chatgpt_url:target,...task});
    if(message.type==='will_submit')post({type:'continue'});
    if(message.type==='started')post({type:'ack'});
    if(['failed','released'].includes(message.type))resolve(message);
  }};
  const sender={id:'test-extension',tab:{id:5},frameId:0,url:senderUrl};
  w.chrome={runtime:{id:'test-extension',connectNative:()=>native,onMessage:{addListener:fn=>{runtimeListener=fn;}}},tabs:{
    create:async data=>{navigations.push(data.url);return {id:5};},
    update:async (id,data)=>{
      assert.equal(id,5);navigations.push(data.url);order.push(`navigate:${data.url}`);
      w.queueMicrotask(()=>runtimeListener({kind:'slingring',event:'ready'},{...sender,url:data.url},()=>{}));
    },
    sendMessage:async (id,message)=>{
      assert.equal(id,5);contentRequests.push(message);order.push(message.action);
      if(message.action==='ensure_chat')return bootstrapError ? {ok:false,code:bootstrapError,submitted:false} : {ok:true};
      if(message.action==='prepare')return {ok:true,effort:null};
      const ack=await new Promise(done=>runtimeListener({kind:'slingring',event:'will_submit',id:'test-job'},sender,done));
      if(!ack?.ok)return {ok:false,submitted:false,code:'submission_not_authorized'};
      return {ok:true,url:resultUrl,evidence:'thinking',effort:null,submitted:true};
    },
    remove:async id=>{removed.push(id);},getCurrent:async()=>({id:99})
  }};
  w.eval(source);
  const terminal=await finished;
  await Promise.resolve();
  const result={terminal,events,navigations,contentRequests,removed,order};
  w.close();return result;
}

test('bridge bootstraps Chat at home, then opens the exact follow-up in the same owned tab',async()=>{
  const r=await run();
  assert.deepEqual(r.navigations,['about:blank','https://chatgpt.com/',target]);
  assert.deepEqual(r.order,['navigate:https://chatgpt.com/','ensure_chat',`navigate:${target}`,'prepare','submit']);
  assert.equal(r.contentRequests.find(m=>m.action==='prepare').chatgpt_url,target);
  const bootstrap=r.contentRequests.find(m=>m.action==='ensure_chat');
  assert.equal(typeof bootstrap.deadline_ms,'number');assert.equal(bootstrap.prompt,undefined);assert.equal(bootstrap.effort,undefined);
  assert.equal(r.contentRequests.filter(m=>m.action==='submit').length,1);
  assert.equal(r.terminal.type,'released');assert(r.removed.includes(5));
  assert.equal(r.events.find(e=>e.type==='started').url,target);
});

test('new_task still navigates home and allows its newly created conversation URL',async()=>{
  const r=await run({task:{chatgpt_url:null},senderUrl:'https://chatgpt.com/'});
  assert.deepEqual(r.navigations,['about:blank','https://chatgpt.com/']);
  assert.equal(r.contentRequests[0].chatgpt_url,null);assert.equal(r.terminal.type,'released');
  assert.equal(r.contentRequests.some(m=>m.action==='ensure_chat'),false);
});

test('failed Chat bootstrap never navigates to the target or prepares a message',async()=>{
  const r=await run({bootstrapError:'chat_mode_unverified'});
  assert.deepEqual(r.navigations,['about:blank','https://chatgpt.com/']);
  assert.deepEqual(r.contentRequests.map(m=>m.action),['ensure_chat']);
  assert.equal(r.terminal.code,'chat_mode_unverified');assert.equal(r.terminal.submitted,false);
  assert.equal(r.events.some(e=>e.type==='will_submit'),false);
});

test('an expired job deadline prevents bootstrap navigation and any content action',async()=>{
  const r=await run({task:{deadline_ms:Date.now()-1}});
  assert.equal(r.terminal.code,'deadline_before_submission');assert.equal(r.terminal.submitted,false);
  assert.equal(r.navigations.length,0);assert.equal(r.contentRequests.length,0);
});

test('bridge rejects malformed targets or follow-up effort/images before creating a browser tab',async()=>{
  for(const task of [{chatgpt_url:target+'\n'},{chatgpt_url:target+'?x=1'},{chatgpt_url:'https://chatgpt.com/c/'+ 'a'.repeat(129)},
    {chatgpt_url:false},{chatgpt_url:''},{effort:'High'},{image_count:1}]) {
    const r=await run({task});assert.equal(r.terminal.type,'failed');assert.equal(r.terminal.code,'invalid_followup');
    assert.equal(r.terminal.submitted,false);assert.equal(r.navigations.length,0);
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
