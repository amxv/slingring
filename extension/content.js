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
  async function handle(message) {
    if (message.action === "prepare") {
      if (task || preparing || submitted) throw Error("duplicate_prepare");
      if (location.origin !== "https://chatgpt.com" || location.pathname !== "/") throw Error("not_new_chat");
      preparing = true;
      await dom.ensureChat();
      if (document.querySelector('main [data-message-author-role],main [data-user-message-bubble]')) throw Error("not_empty_chat");
      const verified = await dom.setEffort(message.effort);
      dom.fill(message.prompt);
      await dom.waitFor(()=>dom.composer() && dom.text(dom.composer())===dom.normal(message.prompt),10000,"prompt_not_verified");
      const selector = message.image_count ? await dom.uploadSelector() : null;
      task={id:message.id,prompt:message.prompt,effort:verified,requested:message.effort,image_count:message.image_count,
        timeout:Math.max(1000,Math.min(message.timeout_ms || 120000,180000))};
      return {ok:true,effort:verified,upload_selector:selector};
    }
    if (message.action === "submit") {
      if (!task || message.id!==task.id || submitted) throw Error("invalid_submission_state");
      await dom.waitFor(()=>dom.attachmentsReady(task.image_count),45000,"images_not_ready");
      const button=await dom.waitFor(dom.sendButton,15000,"send_button_unavailable");
      if (!dom.chatVerified() || location.pathname!=="/") throw Error("chat_mode_unverified");
      if (dom.text(dom.composer())!==dom.normal(task.prompt)) throw Error("prompt_changed");
      if (task.requested && dom.currentEffort()!==task.requested) throw Error("effort_changed");
      const ack=await chrome.runtime.sendMessage({kind:"slingring",event:"will_submit",id:task.id});
      if (ack?.ok!==true) throw Error("submission_not_authorized");
      // Recheck after the native acknowledgement; never click on an assumed state.
      if (!dom.chatVerified() || button.disabled || dom.text(dom.composer())!==dom.normal(task.prompt)) throw Error("page_changed_before_send");
      submitted=true;
      button.click();
      let reported=false;
      const evidence=await dom.waitFor(()=>{
        const url=dom.canonicalUrl();
        if(url && !reported){reported=true;chrome.runtime.sendMessage({kind:"slingring",event:"conversation",id:task.id,url}).catch(()=>{});}
        return dom.startEvidence(task.prompt);
      },task.timeout,"generation_not_started");
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
