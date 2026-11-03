import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {JSDOM} from 'jsdom';
const source=readFileSync(new URL('../dom.js',import.meta.url),'utf8');
function page(html,url='https://chatgpt.com/c/test-conversation') {
  const dom=new JSDOM(`<main>${html}</main>`,{url,runScripts:'outside-only'});
  dom.window.HTMLElement.prototype.getClientRects=function(){
    for(let e=this;e;e=e.parentElement){if(e.hidden||e.style.display==='none')return [];}
    return [{}];
  };
  dom.window.HTMLElement.prototype.getBoundingClientRect=function(){return {top:10,left:10,width:100,height:40};};
  dom.window.eval(source);
  return {window:dom.window,document:dom.window.document,api:dom.window.__slingringDOM,close:()=>dom.window.close()};
}
const controls='<button id="chat" aria-pressed="true">Chat</button><button id="work" aria-pressed="false">Work</button><div contenteditable="true" aria-label="Ask ChatGPT"></div>';
const modern=(body)=>`<div data-thread-user-message-navigation-content><div data-user-message-bubble>Test prompt</div>${body}</div>`;
test('editor serialization preserves paragraph and explicit break semantics without collapsing spaces',()=>{
  const p=page('<div contenteditable="true" aria-label="Ask ChatGPT"></div>'),input=p.api.composer();
  for(const [html,expected] of [['<p>A</p><p>B</p>','A\nB'],['<div>A</div><div>B</div>','A\nB'],
    ['A<br>B','A\nB'],['<p>A</p><p><br></p><p>B</p>','A\n\nB'],['<p><br></p>',''],['  A  B  ','  A  B  '],
    ['<p><br></p><p>B</p>','\nB'],['<p><br></p><p><br></p><p>B</p>','\n\nB'],
    ['<p>A</p><p><br></p>','A\n'],['<p><br></p><p><br></p>','\n'],
    ['<p>A<br class="ProseMirror-trailingBreak"></p><p>B</p>','A\nB']]) {
    input.innerHTML=html;assert.equal(p.api.editorText(input),expected);
    assert.equal(p.api.promptEquals(input,expected),true);
  }
  input.textContent='A  B';assert.equal(p.api.promptEquals(input,'A B'),false);
  input.textContent=' A ';assert.equal(p.api.promptEquals(input,'A'),false);p.close();
});

test('filling rejects an existing identical draft as well as different text',()=>{
  for(const existing of ['test prompt','different prompt','  ']) {
    const p=page('<textarea id="prompt-textarea"></textarea>');p.api.composer().value=existing;
    assert.throws(()=>p.api.fill('test prompt'),/unrelated_draft_present/);
    assert.equal(p.api.composer().value,existing);p.close();
  }
});
test('a stop button, an empty assistant, and a loading dot are not evidence',()=>{
  const p=page(modern('<button aria-label="Stop">Stop</button><div data-markdown-text-style>...</div><div data-message-author-role="assistant"></div>'));
  assert.equal(p.api.startEvidence(),null);p.close();
});
test('new Chat layout reports thinking only outside the user bubble',()=>{
  const p=page(modern('<span>Thinking</span>'));
  assert.equal(p.api.startEvidence(),'thinking');
  p.document.querySelector('span').remove();
  p.document.querySelector('[data-user-message-bubble]').innerHTML='<span>Thinking</span>';
  assert.equal(p.api.startEvidence(),null);p.close();
});
test('thinking and working labels may include meaningful progress detail',()=>{
  for(const [label,kind] of [['Thinking deeply','thinking'],['Working on the problem','working'],['Searching the web','working']]) {
    const p=page(modern(`<span>${label}</span>`));assert.equal(p.api.startEvidence(),kind);p.close();
  }
});
test('new Chat assistant Markdown produces only a boolean classification, never its text',()=>{
  const p=page(modern('<div data-markdown-text-style>fixture response that must not escape</div>'));
  assert.equal(p.api.startEvidence(),'streaming');
  assert(!JSON.stringify(p.api.startEvidence()).includes('fixture'));p.close();
});
test('a retained hidden pre-submit main cannot mask activity in the visible conversation',()=>{
  const p=page('');
  p.document.body.innerHTML=`<main style="display:none">${modern('<button aria-label="Stop">Stop</button>')}</main><main>${modern('<div data-markdown-text-style>fixture response</div>')}</main>`;
  assert.equal(p.api.startEvidence(),'streaming');p.close();
});
test('legacy Chat layout still recognizes real response content',()=>{
  const p=page('<div data-message-author-role="user">fixture prompt</div><div data-message-author-role="assistant"><div class="markdown">fixture response</div></div>');
  assert.equal(p.api.startEvidence(),'streaming');p.close();
});
test('no canonical URL or no user message means no start confirmation',()=>{
  const p=page(modern('<span>Thinking</span>'),'https://chatgpt.com/');
  assert.equal(p.api.startEvidence(),null);p.close();
  const q=page('<span>Thinking</span><div data-markdown-text-style>fixture</div>');
  assert.equal(q.api.startEvidence(),null);q.close();
});
test('Work mode is never considered Chat, including an ambiguous switch',()=>{
  const p=page(controls,'https://chatgpt.com/');
  assert.equal(p.api.chatVerified(),true);
  p.document.querySelector('#chat').setAttribute('aria-pressed','false');
  p.document.querySelector('#work').setAttribute('aria-pressed','true');
  assert.equal(p.api.chatVerified(),false);
  p.document.querySelector('#chat').removeAttribute('aria-pressed');
  assert.equal(p.api.chatVerified(),false);p.close();
});
test('an existing thread with only the known Ask ChatGPT editor is positively recognized',()=>{
  const p=page('<div contenteditable="true" aria-label="Ask ChatGPT"></div>');
  assert.equal(p.document.querySelector('#prompt-textarea'),null);
  assert.equal(p.api.modes().chat,null);assert.equal(p.api.modes().work,null);
  assert.equal(p.api.chatVerified(),true);p.close();
});
test('unrelated editable controls cannot establish Chat mode',()=>{
  for(const html of ['<div contenteditable="true">Ask ChatGPT</div>',
    '<div contenteditable="true" aria-label="Ask another assistant"></div>',
    '<textarea aria-label="Ask ChatGPT"></textarea>']) {
    const p=page(html);assert.equal(p.api.chatVerified(),false);p.close();
  }
});
test('visible Work editors and Work composer markers override a known Chat editor',()=>{
  const chat='<div contenteditable="true" aria-label="Ask ChatGPT"></div>';
  for(const work of ['<div contenteditable="true" aria-label="Work with ChatGPT"></div>',
    '<textarea aria-label="Work with ChatGPT"></textarea>','<div data-testid="work-composer"></div>']) {
    const p=page(chat+work);assert.equal(p.api.chatVerified(),false);p.close();
  }
  const p=page(chat+'<div hidden contenteditable="true" aria-label="Work with ChatGPT"></div>');
  assert.equal(p.api.chatVerified(),true);p.close();
});
test('explicit Work or ambiguous mode controls override the known-editor fallback',()=>{
  const editor='<div contenteditable="true" aria-label="Ask ChatGPT"></div>';
  for(const switches of ['<button aria-pressed="false">Chat</button><button aria-pressed="true">Work</button>',
    '<button aria-pressed="true">Chat</button><button>Chat</button><button aria-pressed="false">Work</button>',
    '<button>Chat</button><button aria-pressed="false">Work</button>']) {
    const p=page(switches+editor);assert.equal(p.api.chatVerified(),false);p.close();
  }
  const p=page(editor,'https://chatgpt.com/work/c/test');assert.equal(p.api.chatVerified(),false);p.close();
});
test('hydrating disabled Chat control is awaited, rather than sent through or rejected early',async()=>{
  const p=page(controls,'https://chatgpt.com/');
  const chat=p.document.querySelector('#chat'),work=p.document.querySelector('#work');
  chat.disabled=true;chat.setAttribute('aria-pressed','false');work.setAttribute('aria-pressed','true');
  chat.addEventListener('click',()=>{chat.setAttribute('aria-pressed','true');work.setAttribute('aria-pressed','false');});
  p.window.setTimeout(()=>{chat.disabled=false;},10);
  await p.api.ensureChat();assert.equal(p.api.chatVerified(),true);p.close();
});
test('home bootstrap never switches away from an actual Work draft or attachment',async()=>{
  for(const extra of ['<div contenteditable="true" aria-label="Work with ChatGPT"><p>Existing user draft</p></div>',
    '<div contenteditable="true" aria-label="Work with ChatGPT"><p>  </p></div>',
    '<div contenteditable="true" aria-label="Work with ChatGPT"><p><br></p><p><br></p></div>',
    '<div contenteditable="true" aria-label="Work with ChatGPT"><p><br></p></div><button aria-label="Remove file">Remove</button>']) {
    const p=page('<button aria-pressed="false">Chat</button><button aria-pressed="true">Work</button>'+extra,'https://chatgpt.com/');
    p.document.querySelector('button').addEventListener('click',()=>assert.fail('must not switch away from Work draft'));
    await assert.rejects(p.api.ensureChat(),/unrelated_draft_present|attachments_present/);p.close();
  }
});
test('a structural empty Work paragraph permits the explicit Chat bootstrap',async()=>{
  const p=page('<button id="chat" aria-pressed="false">Chat</button><button id="work" aria-pressed="true">Work</button><div id="editor" contenteditable="true" aria-label="Work with ChatGPT"><p><br></p></div>','https://chatgpt.com/');
  p.document.querySelector('#chat').addEventListener('click',()=>{
    p.document.querySelector('#chat').setAttribute('aria-pressed','true');
    p.document.querySelector('#work').setAttribute('aria-pressed','false');
    p.document.querySelector('#editor').setAttribute('aria-label','Ask ChatGPT');
  });
  await p.api.ensureChat();assert.equal(p.api.chatVerified(),true);assert.equal(p.api.hasDraft(),false);p.close();
});
function picker(levels,start) {
  const p=page(controls+`<button id="model" aria-label="Select ChatGPT model">Thinking effort${levels[start]}</button>
    <div role="menu" style="display:none"><div data-reasoning-slider="true" aria-describedby="status">
    <span role="slider" aria-hidden="true" aria-valuemin="0" aria-valuemax="${levels.length-1}" aria-valuenow="${start}"></span></div>
    <span id="status" role="status" aria-live="polite">${levels[start]}, ${start+1} of ${levels.length}.</span></div>`,'https://chatgpt.com/');
  const menu=p.document.querySelector('[role="menu"]'),model=p.document.querySelector('#model'),control=p.document.querySelector('[data-reasoning-slider]');
  let current=start;
  model.addEventListener('click',()=>{menu.style.display='block';});
  control.addEventListener('keydown',event=>{
    if(event.key==='Escape'){menu.style.display='none';return;}
    current=Math.min(levels.length-1,Math.max(0,current+(event.key==='ArrowLeft'?-1:event.key==='ArrowRight'?1:0)));
    control.querySelector('[role="slider"]').setAttribute('aria-valuenow',String(current));
    p.document.querySelector('#status').textContent=`${levels[current]}, ${current+1} of ${levels.length}.`;
    model.textContent=`Thinking effort${levels[current]}`;
  });
  // jsdom has no layout: model the closed popup's actual visibility.
  p.window.HTMLElement.prototype.getClientRects=function(){return this.closest('[role="menu"]')?.style.display==='none'?[]:[{}];};
  return p;
}
test('all five reasoning levels are chosen and read back on the five-position Chat picker',async()=>{
  const levels=['Instant','Medium','High','Extra High','Pro'];
  const p=picker(levels,4);
  for(const effort of levels){assert.equal(await p.api.setEffort(effort),effort);assert.equal(p.api.currentEffort(),effort);}
  p.close();
});
test('explicit follow-up effort switches the picker without checking Chat/Work mode',async()=>{
  const levels=['Instant','Medium','High','Extra High','Pro'];
  const p=picker(levels,1);
  p.document.querySelector('#chat').setAttribute('aria-pressed','false');
  p.document.querySelector('#work').setAttribute('aria-pressed','true');
  assert.equal(p.api.chatVerified(),false);
  assert.equal(await p.api.ensureEffort('Extra High',Date.now()+8000,{verifyChat:false}),'Extra High');
  assert.equal(p.api.currentEffort(),'Extra High');
  p.close();
});
test('Plus cannot silently downgrade Extra High or Pro',async()=>{
  const p=picker(['Instant','Medium','High'],1);
  await assert.rejects(p.api.setEffort('Extra High'),/effort_unavailable_on_account/);p.close();
});
test('a disabled reasoning picker is awaited before a single click',async()=>{
  const p=picker(['Instant','Medium','High','Extra High','Pro'],3);
  const model=p.document.querySelector('#model');
  model.disabled=true;
  let clicks=0;model.addEventListener('click',()=>clicks++);
  p.window.setTimeout(()=>{model.disabled=false;},10);
  assert.equal(await p.api.setEffort('Medium'),'Medium');
  assert.equal(clicks,1);p.close();
});
test('a six-position Work-like picker is rejected even if its labels resemble Chat',async()=>{
  const p=picker(['Instant','Light','Medium','High','Extra High','Pro'],4);
  await assert.rejects(p.api.setEffort('Extra High'),/effort_unavailable_on_account/);p.close();
});
