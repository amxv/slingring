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
  const EFFORTS=['Instant','Medium','High','Extra High','Pro'];
  const canonical = url => typeof url==='string' && /^https:\/\/chatgpt\.com\/c\/[a-zA-Z0-9-]{1,128}(?![\s\S])/.test(url);
  const atTarget = url => url ? location.href===url : location.origin==='https://chatgpt.com' && location.pathname==='/';
  const progress=(id,stage)=>chrome.runtime.sendMessage({kind:'slingring',event:'progress',id,stage}).catch(()=>{});
  // Only allowlisted reasoning labels and observation states cross the native
  // bridge. Never send model-menu text, DOM output, prompts or conversation URLs.
  const effortObservation=(id,phase,expected)=>{
    if(!expected)return null;
    const state=dom.effortState();
    chrome.runtime.sendMessage({kind:'slingring',event:'effort_state',id,phase,expected,
      observed:state.value,kind:state.kind}).catch(()=>{});
    return state.value;
  };
  async function ensureRequestedEffort(id,phase,expected,deadline,followup) {
    if(!expected)return null;
    const observed=effortObservation(id,phase,expected);
    if(observed===expected)return expected;
    // Voice readiness already established that the conversation is loaded.
    // No visible/recognizable model control? Don't spend extra seconds polling
    // an unrelated selector. Send without claiming a verified effort change.
    if(observed===null)return null;
    // An explicit override is best-effort. A missing or changing picker is not
    // grounds to block an otherwise ready conversation. Omitted overrides do
    // not even call this function or inspect the effort UI.
    progress(id,'effort_selection');
    try {
      await dom.ensureEffort(expected,Math.min(deadline,Date.now()+6500),{verifyChat:!followup});
    } catch { /* No retry or block on an unavailable/rehydrating effort UI. */ }
    return effortObservation(id,phase,expected)===expected ? expected : null;
  }
  function guardTarget(url) {
    if (!atTarget(url)) throw Error(url ? 'conversation_changed' : 'not_new_chat');
  }
  function guardPrepared(checkUploaded=false,allowUpload=false) {
    if (!dom.composer()) throw Error('composer_unavailable');
    if (!dom.followupIdle(allowUpload)) throw Error('conversation_busy');
    if (!task.image_count && !dom.noAttachments()) throw Error('attachments_present');
    if (checkUploaded && (!dom.attachmentsReady(task.image_count) ||
        (task.image_count && !dom.attachmentsUnchanged(task.attachments)))) throw Error('attachments_changed');
  }
  async function handle(message) {
    if (message.action === "prepare") {
      if (task || preparing || submitted) throw Error("duplicate_prepare");
      const target=message.chatgpt_url ?? null;
      if (target!==null && !canonical(target)) throw Error('invalid_chat_url');
      if (message.effort!=null && !EFFORTS.includes(message.effort)) throw Error('invalid_effort');
      guardTarget(target);
      preparing = true;
      const deadline=message.deadline_ms || Date.now()+120000;
      let verified;
      if (target) {
        const available=Math.min(message.timeout_ms || 120000,deadline-Date.now()-5000);
        if (available<=0) throw Error('deadline_before_submission');
        await dom.waitFor(()=>{guardTarget(target);return dom.composer();},Math.min(15000,available),'composer_unavailable');
        await dom.waitFor(()=>{
          guardTarget(target);
          // Exact URL, known Chat composer, an idle voice control, and no stop
          // signal establish readiness. Never visit home or toggle Chat/Work.
          return dom.followupReady();
        },available,'conversation_not_ready');
        guardTarget(target);
        if (!dom.followupReady()) throw Error('conversation_not_ready');
        verified=await ensureRequestedEffort(message.id,'prepare',message.effort,deadline,true);
      } else {
        progress(message.id,'chat_mode_verification');
        await dom.ensureChat(deadline);
        if ([...document.querySelectorAll('main [data-message-author-role],main [data-user-message-bubble]')].some(dom.visible)) throw Error("not_empty_chat");
        if(dom.hasDraft())throw Error('unrelated_draft_present');
        if(!dom.noAttachments())throw Error('attachments_present');
        progress(message.id,'effort_selection');
        verified=await dom.setEffort(message.effort);
      }
      guardTarget(target);
      progress(message.id,'composer_ready');
      await dom.stableComposer(deadline);
      guardTarget(target);
      if(!dom.noAttachments())throw Error('attachments_present');
      if(target && !dom.followupReady())throw Error('conversation_not_ready');
      progress(message.id,'prompt_filling');
      dom.fill(message.prompt);
      progress(message.id,'prompt_verification');
      await dom.waitFor(()=>dom.promptEquals(dom.composer(),message.prompt),10000,"prompt_not_verified");
      // Let the editor hydrate its document model before capturing an owned
      // prompt. A genuinely lost or changed draft is never refilled here.
      await new Promise(resolve=>setTimeout(resolve,150));
      const state=dom.promptState(message.prompt);
      if(state!=='matches')throw Error(state);
      const selector = message.image_count ? await dom.uploadSelector() : null;
      task={id:message.id,prompt:message.prompt,effort:verified,requested:message.effort,image_count:message.image_count,
        target,deadline,
        timeout:Math.max(1000,Math.min(message.timeout_ms || 120000,3600000))};
      return {ok:true,effort:verified,upload_selector:selector};
    }
    if (message.action === "submit") {
      if (!task || message.id!==task.id || submitted) throw Error("invalid_submission_state");
      guardTarget(task.target);
      guardPrepared(false,task.image_count>0);
      progress(task.id,'attachment_wait');
      let attachments=null;
      await dom.waitFor(()=>{
        guardTarget(task.target);guardPrepared(false,task.image_count>0);
        const state=dom.promptState(task.prompt);if(state!=='matches')throw Error(state);
        if(!dom.attachmentsReady(task.image_count))return false;
        attachments=task.image_count ? dom.watchAttachments() : null;
        return !task.image_count || !!attachments;
      },Math.max(1,Math.min(45000,task.deadline-Date.now()-5000)),"images_not_ready");
      task.attachments=attachments;
      guardPrepared(true);
      await dom.waitFor(dom.sendButton,15000,"send_button_unavailable");
      guardTarget(task.target);
      guardPrepared(true);
      if(!task.target && !dom.chatVerified())throw Error('chat_mode_unverified');
      const state=dom.promptState(task.prompt);if(state!=='matches')throw Error(state);
      if (task.deadline-Date.now()<5000) throw Error("deadline_before_submission");
      progress(task.id,'send_ready');
      const ack=await chrome.runtime.sendMessage({kind:"slingring",event:"will_submit",id:task.id});
      if (ack?.ok!==true) throw Error("submission_not_authorized");
      // React may replace the composer/send control while the native round trip
      // is pending. Re-query after the acknowledgement, never click a detached
      // element whose disabled property still happens to be false.
      const button=dom.sendButton();
      const input=dom.composer();
      guardTarget(task.target);
      guardPrepared(true);
      if (task.deadline-Date.now()<5000) throw Error('deadline_before_submission');
      if (!button?.isConnected || !input?.isConnected || (!task.target && !dom.chatVerified()) || button.disabled ||
          !dom.promptEquals(input,task.prompt)) throw Error("page_changed_before_send");
      const baseline=dom.turnBaseline();
      submitted=true;
      button.click();
      progress(task.id,'activity_wait');
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
      submitted,url:dom.canonicalUrl()})).finally(()=>{if(message.action==='submit')dom.releaseAttachments?.(task?.attachments);});
    return true;
  });
  chrome.runtime.sendMessage({kind:"slingring",event:"ready"}).catch(()=>{});
})();
