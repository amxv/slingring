/* Shared DOM adapter, loaded in Chrome's isolated content-script world. */
(() => {
  "use strict";
  const LEVELS = ["Instant", "Medium", "High", "Extra High", "Pro"];
  const SELECTOR = '[contenteditable="true"][aria-label="Ask ChatGPT"],#prompt-textarea,textarea[data-testid="prompt-textarea"]';
  const visible = e => !!e && !e.closest('[inert],[aria-hidden="true"]') &&
    getComputedStyle(e).display !== "none" && getComputedStyle(e).visibility !== "hidden" && e.getClientRects().length > 0;
  const normal = text => String(text || "").replace(/\r\n/g, "\n").trim();
  function composer() { return [...document.querySelectorAll(SELECTOR)].find(visible); }
  function text(e) { return normal("value" in e ? e.value : e.innerText || e.textContent); }
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
    const m = modes();
    if (m.ambiguous) return false;
    if (m.chat || m.work) return selected(m.chat) === true && selected(m.work) === false;
    // Classic Chat-only accounts have no mode switch. Unknown layouts fail closed.
    return visible(document.querySelector('#prompt-textarea')) && !document.querySelector('[data-testid*="work-composer"]');
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
  async function ensureChat() {
    for (let attempt=0; attempt<3; attempt++) {
      const state = await waitFor(() => {
        if (chatVerified()) return {ready:true};
        const m=modes();
        return m.chat && m.work && !m.ambiguous && !m.chat.disabled && m.chat.getAttribute('aria-disabled')!=='true'
          ? {button:m.chat} : null;
      }, 45000, "chat_mode_unverified");
      if (state.button?.isConnected) state.button.click();
      await waitFor(chatVerified,15000,"chat_mode_unverified");
      await waitFor(composer,30000,"composer_unavailable");
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
  function currentEffort() {
    const label = normal(modelButton()?.textContent);
    return [...LEVELS].sort((a,b)=>b.length-a.length).find(e => label.endsWith(e)) || null;
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
  async function setEffort(requested) {
    if (!requested) return currentEffort();
    if (!LEVELS.includes(requested)) throw Error("invalid_effort");
    if (!chatVerified()) throw Error("chat_mode_unverified");
    let state = powerState();
    for (let attempt=0; !state && attempt<3; attempt++) {
      const button=await waitFor(()=>{
        const e=modelButton();return e?.isConnected && !e.disabled && e.getAttribute('aria-disabled')!=='true' ? e : null;
      },15000,"effort_picker_unavailable");
      if (!chatVerified()) throw Error("chat_mode_unverified");
      // A fresh React page can replace the first button before its handlers are
      // hydrated. Retry only opening the menu, never submitting the prompt.
      key(button,"Escape");
      button.click();
      try { state=await waitFor(powerState,8000,"effort_picker_unavailable"); }
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
      if (!chatVerified()) throw Error("chat_mode_unverified");
      const old = state.value;
      state.control.focus(); key(state.control,old>desired ? "ArrowLeft" : "ArrowRight");
      state = await waitFor(() => {
        const next = powerState(); return next && next.value !== old ? next : null;
      },5000,"effort_selection_failed");
    }
    if (state.label !== requested || state.value !== desired) throw Error("effort_selection_failed");
    key(state.control,"Escape");
    await waitFor(() => !powerState(),5000,"effort_picker_did_not_close");
    if (currentEffort() !== requested) throw Error("effort_not_verified");
    return requested;
  }
  function fill(prompt) {
    const input = composer();
    if (!input) throw Error("composer_unavailable");
    const existing = text(input);
    if (existing && existing !== normal(prompt)) throw Error("unrelated_draft_present");
    if (existing === normal(prompt)) return;
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
    if(input.getAttribute('aria-label')==='Attach photos')return 'input[type="file"][multiple][aria-label="Attach photos"]';
    return input.accept ? `input[type="file"][multiple][accept="${CSS.escape(input.accept)}"]` : 'input[type="file"][multiple]';
  }
  function attachmentsReady(count) {
    if (!count) return true;
    const input = composer(); const root = input?.closest("form") || input?.closest("main");
    if (!root) return false;
    const previews = [...root.querySelectorAll('button[aria-label*="Remove"],button[aria-label*="remove"],[data-testid="file-thumbnail"]')].filter(visible);
    const busy = [...root.querySelectorAll('[role="progressbar"],[aria-busy="true"]')].some(visible);
    return previews.length >= count && !busy && !!sendButton();
  }
  function canonicalUrl() {
    return location.origin === "https://chatgpt.com" && /^\/c\/[a-zA-Z0-9-]+$/.test(location.pathname)
      ? location.origin+location.pathname : null;
  }
  function activityLabel(element) {
    if (!visible(element) || element.closest('.sr-only,[aria-hidden="true"]') || element.querySelector('.pulsing-dot')) return '';
    const clone=element.cloneNode(true);
    for(const hidden of clone.querySelectorAll('.sr-only,[aria-hidden="true"],svg')) hidden.remove();
    return normal(clone.textContent || (element.tagName==='BUTTON' ? element.getAttribute('aria-label') : ''));
  }
  function startEvidence(expectedPrompt) {
    const user = [...document.querySelectorAll('[data-message-author-role="user"],[data-user-message-bubble]')]
      .find(e=>visible(e) && (!expectedPrompt || normal(e.textContent)===normal(expectedPrompt)));
    if (!user || !canonicalUrl()) return null;
    // ChatGPT can retain a home view beside its live thread. Anchor observation
    // to this task's user bubble rather than the first <main> on the page.
    const main = user.closest('[data-thread-user-message-navigation-content]') || user.closest('main,[role="main"]');
    if (!main) return null;
    const outsideUser=e=>!e.closest('[data-user-message-bubble],[data-message-author-role="user"],form,nav,header,[data-composer-body]');
    const meaningful=e=>visible(e) && outsideUser(e) && /[^\s.…·●•○◌]/u.test(e.textContent || '');
    const alerts = [...main.querySelectorAll('[role="alert"]')].filter(visible);
    if (alerts.some(e=>/something went wrong|usage limit|network error|try again/i.test(e.textContent))) throw Error("generation_error");
    const assistant = [...main.querySelectorAll('[data-message-author-role="assistant"],article[data-turn="assistant"]'),
      ...[...main.querySelectorAll('[data-conversation-role="assistant"]')].map(e=>e.parentElement).filter(Boolean)];
    for (const area of assistant) {
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
        if(!outsideUser(control) || control.childElementCount > 1 || control.querySelector('[data-user-message-bubble]'))continue;
        const label=activityLabel(control);
        if(label.length < 120 && /^(Thinking|Thought|Reasoning)(?:\b|…)/i.test(label))return 'thinking';
        if(label.length < 120 && /^(Working|Analyzing|Searching|Reading|Running|Browsing)(?:\b|…)/i.test(label))return 'working';
      }
      if([...main.querySelectorAll('[data-markdown-text-style]')].some(meaningful))return 'streaming';
    }
    // Some Chat layouts render the thinking control alongside, not inside, the
    // assistant message. Exclude the composer, user messages and navigation.
    for (const control of main.querySelectorAll('button,[role="status"]')) {
      if (!visible(control) || control.closest('form,[data-message-author-role="user"],[data-user-message-bubble],nav,header') ||
          control.querySelector('.pulsing-dot')) continue;
      const label=activityLabel(control);
      if (/^(Thinking|Thought|Reasoning)(\.{0,3}|…| for .*)?$/i.test(label)) return "thinking";
      if (/^(Working|Analyzing|Searching)(\.{0,3}|…| for .*)?$/i.test(label)) return "working";
    }
    return null; // A stop control, loading dot, or empty assistant node is not evidence.
  }
  globalThis.__slingringDOM = {LEVELS,visible,normal,composer,text,modes,chatVerified,waitFor,ensureChat,
    currentEffort,powerState,setEffort,fill,sendButton,uploadSelector,attachmentsReady,canonicalUrl,startEvidence};
})();
