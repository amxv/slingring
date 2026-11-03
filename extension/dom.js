/* Shared DOM adapter, loaded in Chrome's isolated content-script world. */
(() => {
  "use strict";
  const LEVELS = ["Instant", "Medium", "High", "Extra High", "Pro"];
  const SELECTOR = '[contenteditable="true"][aria-label="Ask ChatGPT"],#prompt-textarea,textarea[data-testid="prompt-textarea"]';
  const visible = e => !!e && !e.closest('[inert],[aria-hidden="true"]') &&
    getComputedStyle(e).display !== "none" && getComputedStyle(e).visibility !== "hidden" && e.getClientRects().length > 0;
  const normal = text => String(text || "").replace(/\r\n?/g, "\n").trim();
  function composer() { return [...document.querySelectorAll(SELECTOR)].find(visible); }
  // innerText adds CSS paragraph margins as extra newlines in Chrome. Editors
  // can rehydrate our plain insertion into <p>/<div>/<br> without changing its
  // actual text. Read explicit line boundaries, never collapse whitespace or
  // use rendered line wrapping as evidence that a prompt changed.
  function editorText(root) {
    if (!root) return null;
    if ('value' in root) return root.value.replace(/\r\n?/g,'\n');
    const block=e=>e.nodeType===Node.ELEMENT_NODE && /^(P|DIV|LI|PRE|BLOCKQUOTE|H[1-6])$/.test(e.tagName);
    function read(parent) {
      const children=[...parent.childNodes].filter(child=>!(child===parent.lastChild && child.nodeName==='BR' && child.classList.contains('ProseMirror-trailingBreak')));
      // A lone <br> is the conventional placeholder in an empty editor/line.
      if(children.length===1 && children[0].nodeName==='BR')return '';
      let value='',previousBlock=false,started=false;
      for(const child of children) {
        const isBlock=block(child);
        // Boundaries depend on structure, including empty leading/trailing
        // paragraphs. Looking only at accumulated text drops blank lines.
        if((isBlock || previousBlock) && started)value+='\n';
        if(child.nodeType===Node.TEXT_NODE)value+=child.nodeValue;
        else if(child.nodeName==='BR')value+='\n';
        else if(child.nodeType===Node.ELEMENT_NODE)value+=read(child);
        previousBlock=isBlock;
        started=true;
      }
      return value;
    }
    return read(root).replace(/\r\n?/g,'\n');
  }
  function text(e) { return normal(editorText(e)); }
  function hasDraft() { return (editorText(composer()) || '').length>0; }
  function promptEquals(input,prompt) { return editorText(input)===String(prompt).replace(/\r\n?/g,'\n'); }
  function promptState(prompt) {
    const input=composer();
    if(!input)return 'composer_unavailable';
    if(promptEquals(input,prompt))return 'matches';
    return hasDraft() ? 'prompt_changed' : 'prompt_lost';
  }
  async function stableComposer(deadline=Date.now()+15000) {
    for(let attempt=0;attempt<3;attempt++) {
      const input=await waitFor(composer,Math.max(1,Math.min(10000,deadline-Date.now()-5000)),'composer_unavailable');
      if(hasDraft())throw Error('unrelated_draft_present');
      await new Promise(resolve=>setTimeout(resolve,150));
      if(hasDraft())throw Error('unrelated_draft_present');
      if(input.isConnected && input===composer())return input;
    }
    throw Error('composer_unstable');
  }
  function modes() {
    const controls = [...document.querySelectorAll('button,[role="tab"],[role="radio"]')]
      .filter(e => visible(e) && e.getBoundingClientRect().top < 300);
    const chat = controls.filter(e => normal(e.textContent) === "Chat");
    const work = controls.filter(e => normal(e.textContent) === "Work");
    return { chat: chat.length === 1 ? chat[0] : null, work: work.length === 1 ? work[0] : null,
      ambiguous: chat.length > 1 || work.length > 1 };
  }
  function selected(e) {
    if (!e) return null;
    for (const key of ["aria-pressed", "aria-selected", "aria-checked", "data-active", "data-selected"]) {
      if (e.getAttribute(key) === "true") return true;
      if (e.getAttribute(key) === "false") return false;
    }
    if (["active", "on", "checked"].includes(e.getAttribute("data-state"))) return true;
    if (["inactive", "off", "unchecked"].includes(e.getAttribute("data-state"))) return false;
    return null;
  }
  function chatVerified() {
    if (location.origin !== "https://chatgpt.com" || /^\/work(?:\/|$)/.test(location.pathname)) return false;
    const workEditor=[...document.querySelectorAll('[contenteditable][aria-label],textarea[aria-label],input[aria-label]')]
      .some(e=>visible(e) && /^work with chatgpt$/i.test(normal(e.getAttribute('aria-label'))));
    if (workEditor || document.querySelector('[data-testid*="work-composer"]')) return false;
    const m = modes();
    if (m.ambiguous) return false;
    if (m.chat || m.work) return selected(m.chat) === true && selected(m.work) === false;
    // Existing Chat threads can omit both the mode switch and the classic ID.
    // Recognize only known Chat editors; unrelated editable controls fail closed.
    return [...document.querySelectorAll('#prompt-textarea,[contenteditable="true"][aria-label="Ask ChatGPT"]')].some(visible);
  }
  function waitFor(predicate, timeout = 30000, code = "ui_timeout") {
    return new Promise((resolve, reject) => {
      let settled = false;
      let scheduled = false;
      const done = (value, error) => {
        if (settled) return;
        settled = true; observer.disconnect(); clearTimeout(timer);
        if (error) reject(error); else resolve(value);
      };
      const check = () => {
        scheduled = false;
        if (settled) return;
        try { const value = predicate(); if (value) done(value); }
        catch (error) { done(null, error); }
      };
      const observer = new MutationObserver(() => {
        if (!scheduled) { scheduled = true; queueMicrotask(check); }
      });
      const timer = setTimeout(() => done(null, new Error(code)), timeout);
      observer.observe(document.documentElement, {subtree:true, childList:true, characterData:true, attributes:true});
      check();
    });
  }
  async function ensureChat(deadline=Date.now()+120000) {
    const budget=limit=>Math.max(1,Math.min(limit,deadline-Date.now()-5000));
    for (let attempt=0; attempt<3; attempt++) {
      if(deadline-Date.now()<5000)throw Error('deadline_before_submission');
      const state = await waitFor(() => {
        if (chatVerified()) return {ready:true};
        const m=modes();
        return m.chat && m.work && !m.ambiguous && !m.chat.disabled && m.chat.getAttribute('aria-disabled')!=='true'
          ? {button:m.chat} : null;
      }, budget(30000), "chat_mode_unverified");
      if(hasDraft())throw Error('unrelated_draft_present');
      if(state.button) {
        const workEditors=[...document.querySelectorAll('[contenteditable][aria-label],textarea[aria-label]')]
          .filter(e=>visible(e) && /^work with chatgpt$/i.test(normal(e.getAttribute('aria-label'))));
        // editorText removes known structural empty-line placeholders. Preserve
        // actual text including literal whitespace; never switch away from it.
        if(workEditors.some(e=>editorText(e).length>0))throw Error('unrelated_draft_present');
        if(workEditors.some(e=>[...(e.closest('form,[data-composer-body],main') || e).querySelectorAll('button[aria-label*="Remove"],button[aria-label*="remove"],[data-testid="file-thumbnail"]')].some(visible)))throw Error('attachments_present');
      }
      if (state.button?.isConnected) state.button.click();
      await waitFor(chatVerified,budget(15000),"chat_mode_unverified");
      await waitFor(composer,budget(15000),"composer_unavailable");
      // Hydration can replace the first controls. This single active-job settle
      // delay mirrors Auto-Send; there is no timer or polling between jobs.
      await new Promise(resolve=>setTimeout(resolve,500));
      if (chatVerified()) return;
    }
    throw Error("chat_mode_unverified");
  }
  function modelButton() {
    return [...document.querySelectorAll('button[aria-label="Select ChatGPT model"],button[data-testid="model-switcher-dropdown-button"]')].find(visible);
  }
  function effortState() {
    const buttons=[...document.querySelectorAll('button[aria-label="Select ChatGPT model"],button[data-testid="model-switcher-dropdown-button"]')].filter(visible);
    if(!buttons.length)return {kind:'missing',value:null};
    if(buttons.length!==1)return {kind:'ambiguous',value:null};
    const label=normal(buttons[0].textContent);
    const value=[...LEVELS].sort((a,b)=>b.length-a.length).find(e=>label.endsWith(e))||null;
    return {kind:value?'known':'unrecognized',value};
  }
  function currentEffort() {
    // Preserve the existing new-chat model readback behavior. The stricter
    // effortState() is only for optional follow-up diagnostics/selection.
    const label=normal(modelButton()?.textContent);
    return [...LEVELS].sort((a,b)=>b.length-a.length).find(e=>label.endsWith(e))||null;
  }
  function powerState() {
    const control = [...document.querySelectorAll('[data-reasoning-slider="true"]')].find(visible);
    if (!control) return null;
    const slider = control.querySelector('[role="slider"]');
    const description = (control.getAttribute("aria-describedby") || "").split(/\s+/)
      .map(id => document.getElementById(id)?.textContent || "").join(" ");
    const m = description.match(/\b(Instant|Medium|High|Extra High|Pro),\s*(\d+)\s+of\s+(\d+)\./i);
    if (!slider || !m) return null;
    return {control, value:Number(slider.getAttribute("aria-valuenow")), maximum:Number(slider.getAttribute("aria-valuemax")),
      label:LEVELS.find(e=>e.toLowerCase()===m[1].toLowerCase()), position:Number(m[2]), count:Number(m[3])};
  }
  function key(element, name) {
    element.dispatchEvent(new KeyboardEvent("keydown", {key:name,code:name,bubbles:true,cancelable:true}));
    element.dispatchEvent(new KeyboardEvent("keyup", {key:name,code:name,bubbles:true,cancelable:true}));
  }
  async function setEffort(requested,{verifyChat=true,deadline=Number.POSITIVE_INFINITY}={}) {
    if (!requested) return currentEffort();
    if (!LEVELS.includes(requested)) throw Error("invalid_effort");
    // Existing canonical /c/ threads have their own target + idle voice
    // guard. Only new-chat creation needs the separate Chat/Work switch check.
    const checkMode=()=>{if(verifyChat && !chatVerified())throw Error('chat_mode_unverified');};
    const budget=limit=>Math.max(1,Math.min(limit,deadline-Date.now()-1500));
    checkMode();
    let state = powerState();
    for (let attempt=0; !state && attempt<3; attempt++) {
      const button=await waitFor(()=>{
        const e=modelButton();return e?.isConnected && !e.disabled && e.getAttribute('aria-disabled')!=='true' ? e : null;
      },budget(15000),"effort_picker_unavailable");
      checkMode();
      // A fresh React page can replace the first button before its handlers are
      // hydrated. Retry only opening the menu, never submitting the prompt.
      key(button,"Escape");
      button.click();
      try { state=await waitFor(powerState,budget(8000),"effort_picker_unavailable"); }
      catch(error) { if(attempt===2 || error.message!=="effort_picker_unavailable")throw error; }
    }
    if (!state) throw Error("effort_picker_unavailable");
    const choices = state.count === 5 ? LEVELS : state.count === 3 ? LEVELS.slice(0,3) : [];
    const desired = choices.indexOf(requested);
    if (desired < 0) throw Error("effort_unavailable_on_account");
    if (state.maximum !== choices.length-1 || choices[state.value] !== state.label || state.position !== state.value+1) {
      throw Error("effort_picker_changed");
    }
    for (let n=0; state.value !== desired && n<5; n++) {
      checkMode();
      const old = state.value;
      state.control.focus(); key(state.control,old>desired ? "ArrowLeft" : "ArrowRight");
      state = await waitFor(() => {
        const next = powerState(); return next && next.value !== old ? next : null;
      },budget(5000),"effort_selection_failed");
    }
    if (state.label !== requested || state.value !== desired) throw Error("effort_selection_failed");
    key(state.control,"Escape");
    await waitFor(() => !powerState(),budget(5000),"effort_picker_did_not_close");
    await waitFor(()=>currentEffort()===requested,budget(2500),"effort_not_verified");
    return requested;
  }
  async function ensureEffort(requested,deadline,{verifyChat=true}={}) {
    if(!requested)return currentEffort();
    if(!LEVELS.includes(requested))throw Error('invalid_effort');
    let observed=effortState();
    if(observed.value===requested) {
      // An initial control may be replaced during hydration; its disappearance
      // is not evidence the user changed reasoning effort.
      await new Promise(resolve=>setTimeout(resolve,100));
      observed=effortState();
      if(observed.value===requested)return requested;
    }
    if(!observed.value) {
      try {
        await waitFor(()=>currentEffort()===requested,Math.max(1,Math.min(1800,deadline-Date.now()-2500)),
          'effort_not_ready');
        return requested;
      } catch(error) {if(error.message!=='effort_not_ready')throw error;}
    }
    return setEffort(requested,{verifyChat,deadline});
  }
  function fill(prompt) {
    const input = composer();
    if (!input) throw Error("composer_unavailable");
    if (hasDraft()) throw Error("unrelated_draft_present");
    input.focus();
    if (input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement) {
      const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto,"value").set.call(input,prompt);
      input.dispatchEvent(new Event("input",{bubbles:true}));
      input.dispatchEvent(new Event("change",{bubbles:true}));
    } else {
      const selection = window.getSelection(); const range = document.createRange();
      range.selectNodeContents(input); selection.removeAllRanges(); selection.addRange(range);
      let inserted = false;
      try { inserted = document.execCommand("insertText",false,prompt); } catch { /* Editor fallback below. */ }
      if (!inserted) {
        input.textContent = prompt;
        input.dispatchEvent(new InputEvent("input",{bubbles:true,inputType:"insertText",data:prompt}));
      }
    }
  }
  function sendButton() {
    const input = composer(); if (!input) return null;
    const root = input.closest("form") || input.closest("main") || document;
    return [...root.querySelectorAll('button[data-testid="send-button"],button[data-testid="composer-submit-button"],button[aria-label="Send"],button[aria-label="Send prompt"],button[aria-label="Send message"]')]
      .find(e=>visible(e) && !e.disabled && e.getAttribute("aria-disabled") !== "true");
  }
  function composerRoot() {
    const input=composer();
    return input?.closest('form') || input?.closest('[data-composer-body]') || input?.closest('main,[role="main"]');
  }
  function stopControl() {
    return [...document.querySelectorAll('button')].some(e=>visible(e) && (
      ['stop-button','composer-stop-button'].includes(e.getAttribute('data-testid')) ||
      /^stop(?: generating| responding| generation| response| streaming)?(?: \(.*\))?$/i.test(normal(e.getAttribute('aria-label')))));
  }
  function voiceActive() {
    return [...document.querySelectorAll('button')].some(e=>visible(e) &&
      /^(?:end voice|exit voice|stop voice|stop recording|stop dictation|stop dictating|mute microphone|unmute microphone)(?:\b|$)/i.test(normal(e.getAttribute('aria-label'))));
  }
  function followupIdle(allowUpload=false) {
    const input=composer(),root=composerRoot();
    return !!input && !!root && !input.disabled && input.getAttribute('aria-disabled')!=='true' &&
      input.getAttribute('contenteditable')!=='false' && !stopControl() && !voiceActive() &&
      (allowUpload || ![...root.querySelectorAll('[aria-busy="true"],[role="progressbar"]')].some(visible));
  }
  function noAttachments() {
    const root=composerRoot();
    return !!root && ![...root.querySelectorAll('button[aria-label*="Remove"],button[aria-label*="remove"],[data-testid="file-thumbnail"]')].some(visible);
  }
  function followupReady() {
    if (!followupIdle()) return false;
    // Never take over an existing draft, even when it happens to match this
    // message. A prior uncertain send may have left it in the composer.
    if (hasDraft()) throw Error('unrelated_draft_present');
    if (!noAttachments()) throw Error('attachments_present');
    const voice=[...composerRoot().querySelectorAll('button')].filter(e=>visible(e) && !e.disabled &&
      e.getAttribute('aria-disabled')!=='true').filter(e=>{
        const label=normal(e.getAttribute('aria-label'));
        return /^(?:start voice|start voice mode|use voice mode|voice mode)$/i.test(label) ||
          (!label && e.getAttribute('data-testid')==='composer-speech-button');
      });
    // A real prior user turn also prevents a still-hydrating or inaccessible
    // conversation from being mistaken for a ready new-chat composer.
    return voice.length===1 && userTurns().length>0;
  }
  function fileInput() {
    const inputs = [...document.querySelectorAll('input[type="file"]')];
    return inputs.find(e=>e.multiple && e.getAttribute('aria-label')==='Attach photos') ||
      inputs.find(e=>e.multiple && /image/i.test(e.accept)) || inputs.find(e=>e.multiple) || null;
  }
  async function uploadSelector() {
    if (!fileInput()) {
      const add = [...document.querySelectorAll('button')].find(e=>visible(e) && /add files|attach files|upload files|add photos/i.test(e.getAttribute('aria-label')||''));
      if (!add) throw Error("image_input_unavailable");
      add.click();
      await waitFor(fileInput,10000,"image_input_unavailable");
    }
    const input = fileInput();
    const marker=crypto.randomUUID();
    input.setAttribute('data-slingring-upload',marker);
    return `input[type="file"][data-slingring-upload="${marker}"]`;
  }
  function attachmentItems(root=composerRoot()) {
    if(!root)return null;
    const allThumbnails=[...root.querySelectorAll('[data-testid="file-thumbnail"]')].filter(visible);
    const thumbnails=allThumbnails.filter(e=>!allThumbnails.some(parent=>parent!==e && parent.contains(e)));
    const removals=[...root.querySelectorAll('button[aria-label*="Remove"],button[aria-label*="remove"]')].filter(visible);
    // A Remove button alone is not an attachment identity: preview/content
    // can be replaced in a sibling without changing that button. Reject such
    // layouts rather than certifying a stable file from a stable control.
    if(removals.length && !thumbnails.length)return null;
    const matched=new Set(),items=[...thumbnails];
    for(const button of removals) {
      const thumbnail=thumbnails.find(e=>e.contains(button));
      if(thumbnail && removals.filter(e=>thumbnail.contains(e)).length!==1)return null;
      // Only the known thumbnail marker establishes one logical card. Generic
      // siblings/ancestors may combine different files and cannot be paired.
      // Mixed unpaired indicators cannot prove any exact cardinality.
      if(!thumbnail && thumbnails.length)return null;
      if(thumbnail) {
        if(matched.has(thumbnail))return null;
        matched.add(thumbnail);
      } else return null;
    }
    return items;
  }
  function attachmentCount(root=composerRoot()) { return attachmentItems(root)?.length ?? null; }
  function watchAttachments() {
    const items=attachmentItems();if(!items)return null;
    const snapshot={items,changed:false,observer:null};
    snapshot.observer=new MutationObserver(()=>{snapshot.changed=true;});
    for(const item of items)snapshot.observer.observe(item,{subtree:true,childList:true,characterData:true,attributes:true});
    return snapshot;
  }
  function attachmentsUnchanged(previous) {
    const current=attachmentItems();
    if(previous?.observer.takeRecords().length)previous.changed=true;
    return previous && !previous.changed && current && current.length===previous.items.length &&
      previous.items.every((element,index)=>element.isConnected && current[index]===element);
  }
  function releaseAttachments(snapshot) { snapshot?.observer.disconnect(); }
  function attachmentsReady(count) {
    if (!count) return true;
    const root=composerRoot();
    if (!root) return false;
    const busy = [...root.querySelectorAll('[role="progressbar"],[aria-busy="true"]')].some(visible);
    return attachmentCount(root) === count && !busy && !!sendButton();
  }
  function canonicalUrl() {
    return location.origin === "https://chatgpt.com" && /^\/c\/[a-zA-Z0-9-]{1,128}$/.test(location.pathname)
      ? location.origin+location.pathname : null;
  }
  function userTurns() {
    const selector='[data-message-author-role="user"],[data-user-message-bubble]';
    return [...document.querySelectorAll(selector)].filter(e=>visible(e) && !e.querySelector(selector));
  }
  function turnBaseline() {
    const users=userTurns();
    return {users};
  }
  function activityLabel(element) {
    if (!visible(element) || element.closest('.sr-only,[aria-hidden="true"]') || element.querySelector('.pulsing-dot')) return '';
    const clone=element.cloneNode(true);
    for(const hidden of clone.querySelectorAll('.sr-only,[aria-hidden="true"],svg')) hidden.remove();
    return normal(clone.textContent || (element.tagName==='BUTTON' ? element.getAttribute('aria-label') : ''));
  }
  function startEvidence(expectedPrompt, baseline) {
    // Sending must consume the submitted input. Appending delayed historical
    // turns can otherwise resemble a new turn while an ignored Send leaves the
    // prompt intact. Recheck the visible current composer on every observation;
    // a missing composer or any draft makes this start ambiguous.
    if (baseline) {
      const input=composer();
      if (!input || text(input)) return null;
    }
    const users=userTurns(),user=users.at(-1);
    // The transcript can contain identical prior prompts and completed answers.
    // Only a newly appended user turn may authorize a start result. If a layout
    // virtualizes/replaces history ambiguously, fail closed rather than infer it.
    if (baseline && (!Array.isArray(baseline.users) || users.length!==baseline.users.length+1 ||
        baseline.users.some((previous,index)=>users[index]!==previous))) return null;
    if (user && expectedPrompt && normal(editorText(user))!==normal(expectedPrompt)) return null;
    if (!user || !canonicalUrl()) return null;
    // ChatGPT can retain a home view beside its live thread. Anchor observation
    // to this task's user bubble rather than the first <main> on the page.
    const main = user.closest('[data-thread-user-message-navigation-content]') || user.closest('main,[role="main"]');
    if (!main) return null;
    const afterUser=e=>!e.contains(user) && !!(user.compareDocumentPosition(e)&Node.DOCUMENT_POSITION_FOLLOWING);
    const outsideUser=e=>!e.closest('[data-user-message-bubble],[data-message-author-role="user"],form,nav,header,[data-composer-body]');
    const meaningful=e=>visible(e) && afterUser(e) && outsideUser(e) && /[^\s.…·●•○◌]/u.test(e.textContent || '');
    const alerts = [...main.querySelectorAll('[role="alert"]')].filter(e=>visible(e) && afterUser(e));
    if (alerts.some(e=>/something went wrong|usage limit|network error|try again/i.test(e.textContent))) throw Error("generation_error");
    const assistant = [...main.querySelectorAll('[data-message-author-role="assistant"],article[data-turn="assistant"]'),
      ...[...main.querySelectorAll('[data-conversation-role="assistant"]')].map(e=>e.parentElement).filter(Boolean)];
    for (const area of assistant) {
      if (!afterUser(area)) continue;
      const status = [...area.querySelectorAll('button,[role="status"],[aria-live],[data-testid*="thinking"],[data-testid*="reasoning"]')]
        .map(activityLabel);
      if (status.some(t=>/^(Thinking|Thought|Reasoning)(?:\b|…)/i.test(t) && t.length<120)) return "thinking";
      if (status.some(t=>/^(Working|Analyzing|Searching|Reading|Running|Browsing)(?:\b|…)/i.test(t) && t.length<120)) return "working";
      // Test for presence only. Never return, log, or serialize generated text.
      if ([...area.querySelectorAll('.markdown,.prose,[data-message-content],[data-markdown-text-style="assistant-message"]')]
        .some(meaningful)) return "streaming";
    }
    if(main.matches('[data-thread-user-message-navigation-content]')) {
      for(const control of main.querySelectorAll('span,div')) {
        if(!afterUser(control) || !outsideUser(control) || control.childElementCount > 1 || control.querySelector('[data-user-message-bubble]'))continue;
        const label=activityLabel(control);
        if(label.length < 120 && /^(Thinking|Thought|Reasoning)(?:\b|…)/i.test(label))return 'thinking';
        if(label.length < 120 && /^(Working|Analyzing|Searching|Reading|Running|Browsing)(?:\b|…)/i.test(label))return 'working';
      }
      if([...main.querySelectorAll('[data-markdown-text-style]')].some(meaningful))return 'streaming';
    }
    // Some Chat layouts render the thinking control alongside, not inside, the
    // assistant message. Exclude the composer, user messages and navigation.
    for (const control of main.querySelectorAll('button,[role="status"]')) {
      if (!visible(control) || !afterUser(control) || control.closest('form,[data-message-author-role="user"],[data-user-message-bubble],nav,header') ||
          control.querySelector('.pulsing-dot')) continue;
      const label=activityLabel(control);
      if (/^(Thinking|Thought|Reasoning)(\.{0,3}|…| for .*)?$/i.test(label)) return "thinking";
      if (/^(Working|Analyzing|Searching)(\.{0,3}|…| for .*)?$/i.test(label)) return "working";
    }
    return null; // A stop control, loading dot, or empty assistant node is not evidence.
  }
  globalThis.__slingringDOM = {LEVELS,visible,normal,composer,text,editorText,hasDraft,promptEquals,promptState,stableComposer,modes,chatVerified,waitFor,ensureChat,
    currentEffort,effortState,powerState,setEffort,ensureEffort,fill,sendButton,uploadSelector,attachmentItems,attachmentCount,watchAttachments,attachmentsUnchanged,releaseAttachments,attachmentsReady,canonicalUrl,startEvidence,
    followupReady,followupIdle,noAttachments,turnBaseline};
})();
