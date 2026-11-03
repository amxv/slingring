import {readFileSync} from 'node:fs';
import {afterEach, describe, expect, it} from 'vitest';
import {Window} from 'happy-dom';

const source=readFileSync(new URL('../../extension/dom.js',import.meta.url),'utf8');
const windows: Window[]=[];
function fixture(markup: string, route='/c/test-conversation') {
  // Only repository-owned code and literal fixtures are evaluated. No website
  // scripts or untrusted markup are loaded into this test VM.
  const window=new Window({url:`https://chatgpt.com${route}`,settings:{enableJavaScriptEvaluation:true,suppressInsecureJavaScriptEnvironmentWarning:true}});
  windows.push(window);
  Object.defineProperty(window.HTMLElement.prototype,'getClientRects',{configurable:true,value:function(this: HTMLElement){return this.closest('[hidden]')?[]:[{width:100,height:20}];}});
  window.document.body.innerHTML=markup;
  window.eval(source);
  return {window,dom:(window as any).__slingringDOM,document:window.document};
}
const user='<div data-user-message-bubble="true">test prompt</div>';
const assistant=(body:string)=>`<div><h4 class="sr-only" data-conversation-role="assistant">ChatGPT said:</h4>${body}</div>`;
const thread=(body:string)=>`<main><div data-thread-user-message-navigation-content>${user}${body}</div></main>`;
afterEach(async()=>{await Promise.all(windows.splice(0).map(window=>window.happyDOM.close()));});

describe('genuine handoff evidence',()=>{
  it('rejects a URL, Stop button and loading dot, including misleading screen-reader text',()=>{
    const {dom}=fixture(thread(assistant('<button aria-label="Stop"></button><span role="status"><span class="sr-only">Thinking</span><span class="pulsing-dot"></span></span>')));
    expect(dom.startEvidence('test prompt')).toBeNull();
  });
  it('rejects empty assistant containers and ellipsis-only placeholders',()=>{
    for(const text of ['', '…', '...', '●']) {
      const {dom}=fixture(thread(assistant(`<div data-markdown-text-style="assistant-message">${text}</div>`)));
      expect(dom.startEvidence('test prompt')).toBeNull();
    }
  });
  it('recognizes real visible thinking and working controls',()=>{
    const thinking=fixture(thread(assistant('<button>Thinking…</button>'))).dom;
    const working=fixture(thread('<button>Working…</button>')).dom;
    expect(thinking.startEvidence('test prompt')).toBe('thinking');
    expect(working.startEvidence('test prompt')).toBe('working');
  });
  it('recognizes the current ChatGPT response container and never returns answer text',()=>{
    const {dom}=fixture(thread(assistant('<div data-markdown-text-style="assistant-message"><p>PRIVATE ANSWER</p></div>')));
    const result=dom.startEvidence('test prompt');
    expect(result).toBe('streaming');
    expect(JSON.stringify(result)).not.toContain('PRIVATE');
  });
  it('anchors to the submitted task rather than a retained home view',()=>{
    const {dom}=fixture('<main><button>Thinking effort Extra High</button></main>'+thread(assistant('<div data-markdown-text-style="assistant-message">answer</div>')));
    expect(dom.startEvidence('test prompt')).toBe('streaming');
    expect(dom.startEvidence('different task')).toBeNull();
  });
  it('does not mistake user markdown or user-authored thinking text for activity',()=>{
    const {dom}=fixture('<main><div data-thread-user-message-navigation-content><div data-user-message-bubble="true"><span>Thinking</span><div class="markdown">test prompt</div></div><button aria-label="Stop"></button></div></main>');
    expect(dom.startEvidence()).toBeNull();
  });
  it('requires the canonical saved-conversation route',()=>{
    const {dom}=fixture(thread(assistant('<div class="markdown">answer</div>')),'/');
    expect(dom.startEvidence('test prompt')).toBeNull();
  });
});

describe('Chat mode and reasoning gates',()=>{
  it('rejects Work and ambiguous selected-state controls',()=>{
    const {dom}=fixture('<button aria-pressed="false">Chat</button><button aria-pressed="true">Work</button><div contenteditable="true" aria-label="Work with ChatGPT"></div>','/');
    expect(dom.chatVerified()).toBe(false);
  });
  it('recognizes explicit Chat selection',()=>{
    const {dom}=fixture('<button aria-pressed="true">Chat</button><button aria-pressed="false">Work</button><div contenteditable="true" aria-label="Ask ChatGPT"></div>','/');
    expect(dom.chatVerified()).toBe(true);
  });
  it('detects five-level and three-level account capabilities independently',()=>{
    for(const count of [3,5]) {
      const {dom}=fixture(`<div data-reasoning-slider="true" aria-describedby="label"><span role="slider" aria-valuenow="1" aria-valuemax="${count-1}"></span></div><span id="label">Medium, 2 of ${count}.</span>`);
      expect(dom.powerState().count).toBe(count);
      expect(dom.powerState().label).toBe('Medium');
    }
  });
});
