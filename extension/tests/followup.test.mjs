import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {JSDOM} from 'jsdom';

const domSource=readFileSync(new URL('../dom.js',import.meta.url),'utf8');
const contentSource=readFileSync(new URL('../content.js',import.meta.url),'utf8');
const target='https://chatgpt.com/c/existing-conversation';
const prompt='Follow-up test prompt';
const voice='<button id="voice" aria-label="Start Voice">Voice</button>';
function page({onAck,onSend,oldPrompt='Earlier prompt',url=target,modernEditor=false}={}) {
  const switches=modernEditor ? '' : '<button id="chat" aria-pressed="true">Chat</button><button id="work" aria-pressed="false">Work</button>';
  const editor=modernEditor ? '<div contenteditable="true" aria-label="Ask ChatGPT"></div><button aria-label="Select ChatGPT model">Thinking effortMedium</button>' : '<textarea id="prompt-textarea"></textarea>';
  const page=new JSDOM(`<main>${switches}
    <div data-message-author-role="user" id="old-user"></div>
    <div data-message-author-role="assistant"><button>Thought for 12 seconds</button><div class="markdown">PRIVATE OLD ANSWER</div></div>
    <form>${editor}${voice}</form></main>`,{url,runScripts:'outside-only'});
  const w=page.window,d=w.document;
  w.HTMLElement.prototype.getClientRects=function(){
    for(let e=this;e;e=e.parentElement){if(e.hidden||e.style.display==='none')return [];}
    return [{}];
  };
  w.HTMLElement.prototype.getBoundingClientRect=()=>({top:10,left:10,width:100,height:40});
  d.querySelector('#old-user').textContent=oldPrompt;
  let listener,clicks=0;
  const events=[];
  const appendUser=text=>{
    const e=d.createElement('div');e.setAttribute('data-message-author-role','user');e.textContent=text;
    d.querySelector('form').before(e);return e;
  };
  const appendThinking=()=>{
    const e=d.createElement('div');e.setAttribute('data-message-author-role','assistant');e.innerHTML='<button>Thinking</button>';
    d.querySelector('form').before(e);
  };
  const input=d.querySelector('textarea,[contenteditable="true"][aria-label="Ask ChatGPT"]');
  input.addEventListener('input',()=>{
    d.querySelector('#voice')?.remove();
    if(!d.querySelector('#send')){
      const send=d.createElement('button');send.type='button';send.id='send';send.setAttribute('data-testid','send-button');send.textContent='Send';
      send.addEventListener('click',()=>{
        clicks++;
        if(onSend)onSend({w,d,appendUser,appendThinking});
        else {appendUser(prompt);appendThinking();if('value' in input)input.value='';else input.textContent='';}
      });
      d.querySelector('form').append(send);
    }
  });
  w.chrome={runtime:{id:'test-extension',onMessage:{addListener:fn=>{listener=fn;}},sendMessage:async message=>{
    events.push(message);
    if(message.event==='will_submit'){
      await onAck?.({w,d});
      return {ok:true};
    }
    return {};
  }}};
  w.eval(domSource);
  const api=w.__slingringDOM,wait=api.waitFor;
  api.waitFor=(predicate,ms,code)=>wait(predicate,Math.min(ms,100),code);
  w.eval(contentSource);
  const sender={id:'test-extension',url:'chrome-extension://test-extension/bridge.html'};
  const request=message=>new Promise(resolve=>listener({kind:'slingring',...message},sender,resolve));
  return {w,d,api,events,request,appendUser,appendThinking,get clicks(){return clicks;},close:()=>w.close(),
    prepare:extra=>request({action:'prepare',id:'task',prompt,chatgpt_url:target,effort:null,image_count:0,...extra}),
    submit:()=>request({action:'submit',id:'task'})};
}

test('follow-up opens the exact existing chat, preserves effort, and confirms only new activity',async()=>{
  const p=page();
  p.api.ensureChat=()=>assert.fail('follow-up must not switch Chat/Work');
  p.api.setEffort=()=>assert.fail('follow-up must not select a model or effort');
  assert.equal((await p.prepare()).ok,true);
  const result=await p.submit();
  assert.equal(result.ok,true);assert.equal(result.url,target);assert.equal(result.evidence,'thinking');
  assert.equal(p.clicks,1);
  assert.equal(p.events.filter(e=>e.event==='will_submit').length,1);
  assert(!JSON.stringify(result).includes('PRIVATE OLD ANSWER'));
  p.close();
});

test('the observed existing-thread editor without mode controls or an ID supports a follow-up',async()=>{
  const p=page({modernEditor:true});
  assert.equal(p.d.querySelector('#prompt-textarea'),null);assert.equal(p.api.modes().chat,null);
  p.api.ensureChat=()=>assert.fail('existing thread must not switch modes');
  p.api.setEffort=()=>assert.fail('existing thread must keep its effort');
  assert.equal((await p.prepare()).ok,true);
  const result=await p.submit();assert.equal(result.ok,true);assert.equal(result.evidence,'thinking');
  assert.equal(result.effort,'Medium');assert.equal(p.clicks,1);assert.equal(p.api.text(p.api.composer()),'');p.close();
});

test('home bootstrap only verifies Chat, without filling a prompt or choosing effort',async()=>{
  const p=page({url:'https://chatgpt.com/'});let switched=0;
  p.api.ensureChat=async()=>{switched++;};
  p.api.fill=()=>assert.fail('bootstrap must not fill a prompt');
  p.api.setEffort=()=>assert.fail('bootstrap must not select effort');
  const result=await p.request({action:'ensure_chat',id:'task',deadline_ms:Date.now()+30000});
  assert.equal(result.ok,true);assert.equal(switched,1);assert.equal(p.d.querySelector('textarea').value,'');
  assert.equal(p.events.some(e=>e.event==='will_submit'),false);
  assert.equal((await p.request({action:'ensure_chat',id:'task'})).code,'invalid_bootstrap_state');p.close();
});

test('home bootstrap rejects existing-thread URLs and expired jobs before any mode switch',async()=>{
  const p=page();p.api.ensureChat=()=>assert.fail('must not switch mode on an existing thread');
  assert.equal((await p.request({action:'ensure_chat',id:'task'})).code,'not_new_chat');p.close();
  const q=page({url:'https://chatgpt.com/'});q.api.ensureChat=()=>assert.fail('expired job must not change mode');
  assert.equal((await q.request({action:'ensure_chat',id:'task',deadline_ms:Date.now()-1})).code,'deadline_before_submission');q.close();
});

test('home bootstrap must finish on home with verified Chat and a live deadline',async()=>{
  for(const [effect,code] of [
    [p=>p.w.history.replaceState(null,'',target),'not_new_chat'],
    [p=>{p.api.chatVerified=()=>false;},'chat_mode_unverified'],
    [p=>{p.w.Date.now=()=>Date.now()+60000;},'deadline_before_submission']]) {
    const p=page({url:'https://chatgpt.com/'});p.api.ensureChat=async()=>effect(p);
    assert.equal((await p.request({action:'ensure_chat',id:'task',deadline_ms:Date.now()+30000})).code,code);
    assert.equal(p.d.querySelector('textarea').value,'');assert.equal(p.clicks,0);p.close();
  }
});

test('stop overrides a visible voice button; follow-up waits until idle without stopping the response',async()=>{
  const p=page(),stop=p.d.createElement('button');stop.setAttribute('aria-label','Stop generating');
  stop.addEventListener('click',()=>assert.fail('must never stop an existing response'));
  p.d.querySelector('form').append(stop);
  const preparing=p.prepare();
  assert.equal(p.d.querySelector('textarea').value,'');
  p.w.setTimeout(()=>stop.remove(),10);
  assert.equal((await preparing).ok,true);assert.equal((await p.submit()).ok,true);
  p.close();
});

test('a busy conversation times out without filling or submitting',async()=>{
  const p=page();p.d.querySelector('form').insertAdjacentHTML('beforeend','<button data-testid="stop-button">Stop</button>');
  const result=await p.prepare();
  assert.equal(result.code,'conversation_not_ready');assert.equal(result.submitted,false);
  assert.equal(p.d.querySelector('textarea').value,'');assert.equal(p.clicks,0);p.close();
});

test('missing, disabled, aria-disabled, hidden, or dictation-only voice controls fail closed',async()=>{
  for(const mutate of [e=>e.remove(),e=>{e.disabled=true;},e=>e.setAttribute('aria-disabled','true'),
    e=>{e.hidden=true;},e=>{e.removeAttribute('data-testid');e.setAttribute('aria-label','Dictate');},
    e=>{e.setAttribute('data-testid','composer-speech-button');e.setAttribute('aria-label','Dictate');}]) {
    const p=page();mutate(p.d.querySelector('#voice'));
    assert.equal((await p.prepare()).code,'conversation_not_ready');
    assert.equal(p.d.querySelector('textarea').value,'');assert.equal(p.clicks,0);p.close();
  }
});

test('an existing draft, including the same message, is never overwritten or submitted',async()=>{
  for(const draft of ['Personal unfinished draft',prompt]) {
    const p=page();p.d.querySelector('textarea').value=draft;
    assert.equal((await p.prepare()).code,'unrelated_draft_present');
    assert.equal(p.d.querySelector('textarea').value,draft);assert.equal(p.clicks,0);p.close();
  }
});

test('attachments and an active voice session prevent a follow-up',async()=>{
  const p=page();p.d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Remove photo">Remove</button>');
  assert.equal((await p.prepare()).code,'attachments_present');assert.equal(p.clicks,0);p.close();
  const q=page();q.d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="End voice">End</button>');
  assert.equal((await q.prepare()).code,'conversation_not_ready');assert.equal(q.clicks,0);q.close();
});

test('Work or an unloaded conversation is never switched or treated as ready',async()=>{
  const p=page(),chat=p.d.querySelector('#chat');chat.setAttribute('aria-pressed','false');
  p.d.querySelector('#work').setAttribute('aria-pressed','true');
  chat.addEventListener('click',()=>assert.fail('mode switched'));
  assert.equal((await p.prepare()).code,'conversation_not_ready');assert.equal(p.clicks,0);p.close();
  const q=page();q.d.querySelector('#old-user').remove();
  assert.equal((await q.prepare()).code,'conversation_not_ready');assert.equal(q.clicks,0);q.close();
});

test('malformed or mismatched follow-up targets fail before filling',async()=>{
  for(const url of [target+'\n',target+'?x=1','https://chatgpt.com/share/abc',target+'#x',
    'https://chatgpt.com/c/'+ 'a'.repeat(129),'https://chatgpt.com.evil.test/c/abc']) {
    const p=page();assert.equal((await p.prepare({chatgpt_url:url})).code,'invalid_chat_url');
    assert.equal(p.d.querySelector('textarea').value,'');assert.equal(p.clicks,0);p.close();
  }
  const q=page({url:target+'?changed=1'});assert.equal((await q.prepare()).code,'conversation_changed');q.close();
});

test('follow-up rejects effort override',async()=>{
  for(const override of [{effort:'Medium'}]) {
    const p=page();assert.equal((await p.prepare(override)).code,'invalid_followup');assert.equal(p.clicks,0);p.close();
  }
});

test('local image follow-up uses existing conversation and waits for an uploaded preview',async()=>{
  const p=page();
  p.d.querySelector('form').insertAdjacentHTML('beforeend',
    '<input type="file" multiple aria-label="Attach photos" accept="image/*">');
  const prepared=await p.prepare({image_count:1});
  assert.equal(prepared.ok,true);
  assert.match(prepared.upload_selector,/^input\[type="file"\]\[data-slingring-upload="[a-f0-9-]{36}"\]$/);
  assert.equal(p.d.querySelectorAll(prepared.upload_selector).length,1);
  assert.equal(p.clicks,0);
  p.d.querySelector('form').insertAdjacentHTML('beforeend',
    '<div data-testid="file-thumbnail"><button aria-label="Remove photo">Remove uploaded photo</button></div>');
  const result=await p.submit();
  assert.equal(result.ok,true);assert.equal(result.url,target);assert.equal(p.clicks,1);
  assert.equal(p.events.filter(e=>e.event==='will_submit').length,1);
  p.close();
});

test('upload progress is awaited without confusing it with a generating response',async()=>{
  const p=page();
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await p.prepare({image_count:1})).ok,true);
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<div id="upload-progress" role="progressbar"></div>');
  const pending=p.submit();assert.equal(p.clicks,0);
  p.w.setTimeout(()=>{
    p.d.querySelector('#upload-progress').remove();
    p.d.querySelector('form').insertAdjacentHTML('beforeend','<div data-testid="file-thumbnail"><button aria-label="Remove photo">Remove</button></div>');
  },20);
  assert.equal((await pending).ok,true);assert.equal(p.clicks,1);p.close();
});

test('an uploaded card with both thumbnail and remove control counts as one image',async()=>{
  const p=page();p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await p.prepare({image_count:1})).ok,true);
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<div data-testid="file-thumbnail"><button aria-label="Remove photo">Remove</button></div>');
  assert.equal(p.api.attachmentCount(),1);assert.equal((await p.submit()).ok,true);assert.equal(p.clicks,1);p.close();
});

test('extra or ambiguous attachments cannot be sent with an expected single image',async()=>{
  for(const html of [
    '<div class="uploaded"><img src="blob:expected"><button aria-label="Remove photo">Remove</button></div>',
    '<div class="uploaded"><img src="blob:expected"><button aria-label="Remove photo">Remove</button></div><div class="replacement"><img src="blob:user"></div>',
    '<button aria-label="Remove expected photo">Remove</button><button aria-label="Remove user file">Remove</button>',
    '<div data-testid="file-thumbnail"></div><button aria-label="Remove user file">Remove</button>',
    '<div class="file-card"><div data-testid="file-thumbnail"></div><button aria-label="Remove photo">Remove</button></div>',
    '<div class="attachments-strip"><div data-testid="file-thumbnail"></div><div class="user-file"><button aria-label="Remove user file">Remove</button></div></div>',
    '<div class="ambiguous-card"><div data-testid="file-thumbnail"></div><button aria-label="Remove photo">Remove</button><button aria-label="Remove another file">Remove</button></div>']) {
    const p=page();p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
    assert.equal((await p.prepare({image_count:1})).ok,true);
    p.d.querySelector('form').insertAdjacentHTML('beforeend',html);
    assert.equal(p.api.attachmentsReady(1),false);
    if(html.includes('file-thumbnail'))assert.equal(p.api.attachmentsReady(2),false,'ambiguous indicators cannot prove two files either');
    assert.equal((await p.submit()).code,'images_not_ready');assert.equal(p.clicks,0);
    assert.equal(p.events.some(e=>e.event==='will_submit'),false);p.close();
  }
});

test('remove-only previews cannot hide sibling replacements behind stable buttons',async()=>{
  const p=page();p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await p.prepare({image_count:1})).ok,true);
  p.d.querySelector('form').insertAdjacentHTML('beforeend',
    '<div class="attachment"><img id="preview" src="blob:expected"><button aria-label="Remove photo">Remove</button></div>');
  assert.equal(p.api.attachmentCount(),null);
  assert.equal(p.api.watchAttachments(),null);
  p.d.querySelector('#preview').setAttribute('src','blob:replacement');
  assert.equal(p.api.attachmentsReady(1),false);
  assert.equal((await p.submit()).code,'images_not_ready');
  assert.equal(p.clicks,0);assert.equal(p.events.some(e=>e.event==='will_submit'),false);
  assert.equal(p.d.querySelector('#preview').getAttribute('src'),'blob:replacement');p.close();
});

test('a user attachment appearing during native authorization is preserved and prevents Send',async()=>{
  const p=page({onAck:({d})=>d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Remove user file">Remove</button>')});
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await p.prepare({image_count:1})).ok,true);
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<div data-testid="file-thumbnail"><button aria-label="Remove photo">Remove</button></div>');
  assert.equal((await p.submit()).code,'attachments_changed');assert.equal(p.clicks,0);
  assert(p.d.querySelector('[aria-label="Remove user file"]'));p.close();
});

test('same-count attachment replacement during native authorization also prevents Send',async()=>{
  const p=page({onAck:({d})=>{
    d.querySelector('[data-testid="file-thumbnail"]').remove();
    d.querySelector('form').insertAdjacentHTML('beforeend','<button aria-label="Remove user replacement">Remove</button>');
  }});
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await p.prepare({image_count:1})).ok,true);
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<div data-testid="file-thumbnail"><button aria-label="Remove photo">Remove</button></div>');
  assert.equal((await p.submit()).code,'attachments_changed');assert.equal(p.clicks,0);
  assert(p.d.querySelector('[aria-label="Remove user replacement"]'));p.close();
});

test('an attachment changed in place during authorization cannot bypass DOM identity checks',async()=>{
  const p=page({onAck:({d})=>d.querySelector('[data-testid="file-thumbnail"] img').setAttribute('src','blob:user-replacement')});
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await p.prepare({image_count:1})).ok,true);
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<div data-testid="file-thumbnail"><img src="blob:expected"><button aria-label="Remove photo">Remove</button></div>');
  assert.equal((await p.submit()).code,'attachments_changed');assert.equal(p.clicks,0);p.close();
});

test('replacing a contained attachment control during authorization also prevents Send',async()=>{
  const p=page({onAck:({d})=>{
    const button=d.querySelector('[data-testid="file-thumbnail"] button');button.replaceWith(button.cloneNode(true));
  }});
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await p.prepare({image_count:1})).ok,true);
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<div data-testid="file-thumbnail"><button aria-label="Remove photo">Remove</button></div>');
  assert.equal((await p.submit()).code,'attachments_changed');assert.equal(p.clicks,0);p.close();
});

test('paragraph rehydration retains exact multiline prompt while real edits abort untouched',async()=>{
  const multiline='Line one\nLine two';
  const p=page({modernEditor:true,onSend:({d,appendUser,appendThinking})=>{
    appendUser(multiline);appendThinking();d.querySelector('[aria-label="Ask ChatGPT"]').textContent='';
  }});
  assert.equal((await p.prepare({prompt:multiline})).ok,true);
  const input=p.api.composer();input.innerHTML='<p>Line one</p><p>Line two</p>';
  Object.defineProperty(input,'innerText',{value:'Line one\n\nLine two',configurable:true});
  assert.equal(p.api.editorText(input),multiline);assert.equal((await p.submit()).ok,true);
  assert.equal(p.clicks,1);p.close();
  for(const [value,code] of [['A real replacement draft','prompt_changed'],['','prompt_lost']]) {
    const q=page();assert.equal((await q.prepare()).ok,true);q.api.composer().value=value;
    assert.equal((await q.submit()).code,code);assert.equal(q.api.composer().value,value);
    assert.equal(q.clicks,0);assert.equal(q.events.some(e=>e.event==='will_submit'),false);q.close();
  }
});

test('whitespace-only existing drafts are preserved and never taken over',async()=>{
  const p=page();p.api.composer().value='  \n ';
  assert.equal((await p.prepare()).code,'unrelated_draft_present');
  assert.equal(p.api.composer().value,'  \n ');assert.equal(p.clicks,0);p.close();
});

test('draft appearing while the empty composer settles is preserved without any fill',async()=>{
  const p=page(),pending=p.prepare();
  p.w.setTimeout(()=>{p.api.composer().value='User typed while loading';},25);
  assert.equal((await pending).code,'unrelated_draft_present');
  assert.equal(p.api.composer().value,'User typed while loading');assert.equal(p.clicks,0);p.close();
});

test('missing conversation composer fails clearly and never fills or authorizes submission',async()=>{
  const p=page();p.api.composer().remove();
  assert.equal((await p.prepare()).code,'composer_unavailable');
  assert.equal(p.events.some(e=>e.event==='will_submit'),false);assert.equal(p.clicks,0);p.close();
});

test('new chat rejects identical saved draft and preserves it',async()=>{
  const p=page({url:'https://chatgpt.com/'});p.api.composer().value=prompt;
  p.d.querySelector('#old-user').remove();p.d.querySelector('[data-message-author-role="assistant"]').remove();
  assert.equal((await p.prepare({chatgpt_url:null})).code,'unrelated_draft_present');
  assert.equal(p.api.composer().value,prompt);assert.equal(p.clicks,0);p.close();
});

test('follow-up images must finish uploading, and cannot disappear during authorization',async()=>{
  const p=page();
  p.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await p.prepare({image_count:1})).ok,true);
  const absent=await p.submit();assert.equal(absent.code,'images_not_ready');
  assert.equal(p.clicks,0);assert.equal(p.events.filter(e=>e.event==='will_submit').length,0);p.close();

  const q=page({onAck:({d})=>d.querySelector('[aria-label="Remove photo"]').remove()});
  q.d.querySelector('form').insertAdjacentHTML('beforeend','<input type="file" multiple aria-label="Attach photos">');
  assert.equal((await q.prepare({image_count:1})).ok,true);
  q.d.querySelector('form').insertAdjacentHTML('beforeend','<div data-testid="file-thumbnail"><button aria-label="Remove photo">Remove</button></div>');
  assert.equal((await q.submit()).code,'attachments_changed');assert.equal(q.clicks,0);q.close();
});

test('navigation before submit or during the native ACK prevents clicking send',async()=>{
  const p=page();assert.equal((await p.prepare()).ok,true);
  p.w.history.replaceState(null,'','/c/another-conversation');
  assert.equal((await p.submit()).code,'conversation_changed');assert.equal(p.clicks,0);
  assert.equal(p.events.filter(e=>e.event==='will_submit').length,0);p.close();
  const q=page({onAck:({w})=>w.history.replaceState(null,'',target+'#changed')});
  assert.equal((await q.prepare()).ok,true);
  assert.equal((await q.submit()).code,'conversation_changed');assert.equal(q.clicks,0);q.close();
});

test('busy state, new attachments, or effort changes during the ACK prevent clicking send',async()=>{
  for(const [html,code] of [
    ['<button data-testid="stop-button">Stop</button>','conversation_busy'],
    ['<button aria-label="Remove file">Remove</button>','attachments_present'],
    ['<button aria-label="Select ChatGPT model">Thinking effortHigh</button>','effort_changed']]) {
    const p=page({onAck:({d})=>d.querySelector('form').insertAdjacentHTML('beforeend',html)});
    assert.equal((await p.prepare()).ok,true);assert.equal((await p.submit()).code,code);assert.equal(p.clicks,0);p.close();
  }
});

test('old answers and old thinking for an identical prompt cannot confirm the follow-up',async()=>{
  const p=page({oldPrompt:prompt,onSend:({appendUser})=>appendUser(prompt)});
  assert.equal((await p.prepare()).ok,true);
  const result=await p.submit();assert.equal(result.code,'generation_not_started');assert.equal(result.submitted,true);
  assert.equal(p.clicks,1);p.close();
  const q=page({oldPrompt:prompt});assert.equal((await q.prepare()).ok,true);
  assert.equal((await q.submit()).evidence,'thinking');assert.equal(q.clicks,1);q.close();
});

test('a rerender of the old transcript does not count as a new turn',async()=>{
  const p=page({oldPrompt:prompt,onSend:({d})=>{
    const old=d.querySelector('#old-user');old.replaceWith(old.cloneNode(true));
  }});
  assert.equal((await p.prepare()).ok,true);assert.equal((await p.submit()).code,'generation_not_started');
  assert.equal(p.clicks,1);p.close();
});

test('append-only historical hydration cannot confirm an ignored Send with unconsumed input',async()=>{
  const p=page({onSend:({appendUser,appendThinking})=>{appendUser(prompt);appendThinking();}});
  assert.equal((await p.prepare()).ok,true);
  const result=await p.submit();
  assert.equal(result.code,'generation_not_started');assert.equal(result.submitted,true);
  assert.equal(p.d.querySelector('textarea').value,prompt);assert.equal(p.clicks,1);p.close();
  const q=page();assert.equal((await q.prepare()).ok,true);
  assert.equal((await q.submit()).evidence,'thinking');assert.equal(q.d.querySelector('textarea').value,'');q.close();
});

test('turn evidence requires a visible empty current composer on every observation',()=>{
  const p=page(),input=p.d.querySelector('textarea'),baseline=p.api.turnBaseline();
  input.value=prompt;p.appendUser(prompt);p.appendThinking();
  assert.equal(p.api.startEvidence(prompt,baseline),null);
  input.value='';assert.equal(p.api.startEvidence(prompt,baseline),'thinking');
  input.value='A different draft';assert.equal(p.api.startEvidence(prompt,baseline),null);
  input.value='';input.hidden=true;assert.equal(p.api.startEvidence(prompt,baseline),null);
  input.remove();assert.equal(p.api.startEvidence(prompt,baseline),null);p.close();
});

test('rehydrated old history with one extra user and a repeated last prompt cannot confirm a new turn',()=>{
  const p=page({oldPrompt:prompt}),baseline=p.api.turnBaseline();
  const old=p.d.querySelector('#old-user');old.replaceWith(old.cloneNode(true));
  p.appendUser(prompt);p.appendThinking();
  assert.equal(p.api.startEvidence(prompt,baseline),null);p.close();
});

test('a native ACK that consumes the remaining submission budget never clicks send',async()=>{
  const p=page({onAck:({w})=>{w.Date.now=()=>future;}}),future=Date.now()+60000;
  assert.equal((await p.prepare({deadline_ms:Date.now()+10000})).ok,true);
  assert.equal((await p.submit()).code,'deadline_before_submission');assert.equal(p.clicks,0);p.close();
});

test('redirects after clicking send produce an uncertain submission, never success',async()=>{
  const p=page({onSend:({w,appendUser,appendThinking})=>{
    appendUser(prompt);appendThinking();w.history.replaceState(null,'','/c/another-conversation');
  }});
  assert.equal((await p.prepare()).ok,true);const result=await p.submit();
  assert.equal(result.code,'conversation_changed');assert.equal(result.submitted,true);assert.equal(p.clicks,1);p.close();
});

test('modern thread containers and hidden pre-submit views retain turn-local evidence',()=>{
  const p=page({oldPrompt:prompt}),baseline=p.api.turnBaseline();
  p.d.querySelector('form').insertAdjacentHTML('beforebegin',`<div data-thread-user-message-navigation-content>
    <div data-user-message-bubble>${prompt}</div><span>Searching the web</span></div>`);
  p.d.body.insertAdjacentHTML('afterbegin','<main hidden><div data-message-author-role="user">Hidden old user</div><div class="markdown">Hidden old answer</div></main>');
  assert.equal(p.api.startEvidence(prompt,baseline),'working');p.close();
});
