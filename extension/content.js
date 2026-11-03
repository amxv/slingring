(() => {
  "use strict";
  if (globalThis.__slingringContentInstalled) return;
  globalThis.__slingringContentInstalled = true;
  const dom = globalThis.__slingringDOM;
  let task = null;
  let submitted = false;
  let preparing = false;
  const trusted = sender => sender.id === chrome.runtime.id &&
    sender.url?.startsWith(`chrome-extension://${chrome.runtime.id}/bridge.html`);
  const canonical = url => typeof url==='string' && /^https:\/\/chatgpt\.com\/c\/[a-zA-Z0-9-]{1,128}(?![\s\S])/.test(url);
  const atTarget = url => url ? location.href===url : location.origin==='https://chatgpt.com' && location.pathname==='/';
  function guardTarget(url) {
    if (!atTarget(url)) throw Error(url ? 'conversation_changed' : 'not_new_chat');
  }
  function guardFollowup() {
    if (!task.target) return;
    if (!dom.followupIdle()) throw Error('conversation_busy');
    if (!dom.noAttachments()) throw Error('attachments_present');
    if (task.effort!==dom.currentEffort()) throw Error('effort_changed');
  }
  async function handle(message) {
    if (message.action === 'ensure_chat') {
      if (task || preparing || submitted) throw Error('invalid_bootstrap_state');
      if (location.href!=='https://chatgpt.com/') throw Error('not_new_chat');
      const deadline=message.deadline_ms || Date.now()+120000;
      if (deadline-Date.now()<5000) throw Error('deadline_before_submission');
      preparing=true;
      // A cold profile may default to Work, where /c/ links fail to load. Select
      // Chat on the empty home page only, before opening the requested thread.
      await dom.ensureChat();
      if (location.href!=='https://chatgpt.com/') throw Error('not_new_chat');
      if (!dom.chatVerified()) throw Error('chat_mode_unverified');
      if (deadline-Date.now()<5000) throw Error('deadline_before_submission');
      return {ok:true};
    }
    if (message.action === "prepare") {
      if (task || preparing || submitted) throw Error("duplicate_prepare");
      const target=message.chatgpt_url ?? null;
      if (target!==null && !canonical(target)) throw Error('invalid_chat_url');
      if (target && (message.effort!=null || message.image_count!==0)) throw Error('invalid_followup');
      guardTarget(target);
      preparing = true;
      const deadline=message.deadline_ms || Date.now()+120000;
      let verified;
      if (target) {
        const available=Math.min(message.timeout_ms || 120000,deadline-Date.now()-5000);
        if (available<=0) throw Error('deadline_before_submission');
        await dom.waitFor(()=>{
          guardTarget(target);
          // A follow-up must keep the existing thread and model. Never click a
          // Chat/Work switch or an effort picker while opening this conversation.
          return dom.chatVerified() && dom.followupReady();
        },available,'conversation_not_ready');
        guardTarget(target);
        if (!dom.chatVerified() || !dom.followupReady()) throw Error('conversation_not_ready');
        verified=dom.currentEffort();
      } else {
        await dom.ensureChat();
        if (document.querySelector('main [data-message-author-role],main [data-user-message-bubble]')) throw Error("not_empty_chat");
        verified=await dom.setEffort(message.effort);
      }
      guardTarget(target);
      dom.fill(message.prompt);
      await dom.waitFor(()=>dom.composer() && dom.text(dom.composer())===dom.normal(message.prompt),10000,"prompt_not_verified");
      const selector = message.image_count ? await dom.uploadSelector() : null;
      task={id:message.id,prompt:message.prompt,effort:verified,requested:message.effort,image_count:message.image_count,
        target,deadline,
        timeout:Math.max(1000,Math.min(message.timeout_ms || 120000,3600000))};
      return {ok:true,effort:verified,upload_selector:selector};
    }
    if (message.action === "submit") {
      if (!task || message.id!==task.id || submitted) throw Error("invalid_submission_state");
      guardTarget(task.target);
      guardFollowup();
      await dom.waitFor(()=>dom.attachmentsReady(task.image_count),45000,"images_not_ready");
      await dom.waitFor(dom.sendButton,15000,"send_button_unavailable");
      guardTarget(task.target);
      guardFollowup();
      if (!dom.chatVerified()) throw Error("chat_mode_unverified");
      if (dom.text(dom.composer())!==dom.normal(task.prompt)) throw Error("prompt_changed");
      if (task.requested && dom.currentEffort()!==task.requested) throw Error("effort_changed");
      if (task.deadline-Date.now()<5000) throw Error("deadline_before_submission");
      const ack=await chrome.runtime.sendMessage({kind:"slingring",event:"will_submit",id:task.id});
      if (ack?.ok!==true) throw Error("submission_not_authorized");
      // React may replace the composer/send control while the native round trip
      // is pending. Re-query after the acknowledgement, never click a detached
      // element whose disabled property still happens to be false.
      const button=dom.sendButton();
      const input=dom.composer();
      guardTarget(task.target);
      guardFollowup();
      if (task.deadline-Date.now()<5000) throw Error('deadline_before_submission');
      if (!button?.isConnected || !input?.isConnected || !dom.chatVerified() || button.disabled ||
          dom.text(input)!==dom.normal(task.prompt) ||
          (task.requested && dom.currentEffort()!==task.requested)) throw Error("page_changed_before_send");
      const baseline=dom.turnBaseline();
      submitted=true;
      button.click();
      let reported=false;
      const evidence=await dom.waitFor(()=>{
        if(task.target)guardTarget(task.target);
        const url=dom.canonicalUrl();
        if(url && !reported){reported=true;chrome.runtime.sendMessage({kind:"slingring",event:"conversation",id:task.id,url}).catch(()=>{});}
        return dom.startEvidence(task.prompt,baseline);
      },Math.max(1,Math.min(task.timeout,task.deadline-Date.now()-2000)),"generation_not_started");
      if(task.target)guardTarget(task.target);
      return {ok:true,url:dom.canonicalUrl(),evidence,effort:task.effort,submitted:true};
    }
    throw Error("unknown_action");
  }
  chrome.runtime.onMessage.addListener((message,sender,reply)=>{
    if (!trusted(sender) || message?.kind!=="slingring") return false;
    handle(message).then(reply).catch(error=>reply({ok:false,code:/^[a-z_]+$/.test(error.message)?error.message:"extension_failed",
      submitted,url:dom.canonicalUrl()}));
    return true;
  });
  chrome.runtime.sendMessage({kind:"slingring",event:"ready"}).catch(()=>{});
})();
