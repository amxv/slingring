/* This page exists only during a job. It never contacts a remote service. */
const HOST = "xyz.ashray.slingring";
const params = new URLSearchParams(location.hash.slice(1));
const token = params.get("token");
const job = params.get("job");
history.replaceState(null,"",location.pathname);
let native;
let tabId;
let targetUrl=null;
let readyUrl=null;
let submitted=false;
let finished=false;
const waits=new Map();
const next=(type,timeout=180000)=>new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>{waits.delete(type);reject(Error("native_timeout"));},timeout);
  waits.set(type,{resolve:value=>{clearTimeout(timer);resolve(value);},reject:error=>{clearTimeout(timer);reject(error);}});
});
function failWaits() { for(const wait of waits.values())wait.reject(Error("native_disconnected"));waits.clear(); }
const safeCode=e=>/^[a-z_]+$/.test(e?.message||"")?e.message:"extension_failed";
const canonical=url=>typeof url==="string" && /^https:\/\/chatgpt\.com\/c\/[a-zA-Z0-9-]{1,128}(?![\s\S])/.test(url);
const stages=new Set(['chat_loading','chat_mode_verification','conversation_loading','composer_ready','effort_selection',
  'prompt_filling','prompt_verification','attachment_wait','send_ready','activity_wait']);
const progress=stage=>native.postMessage({type:'progress',job,stage});
function bootstrapCommitted(id,url) {
  return new Promise((resolve,reject)=>{
    let finished=false;
    const done=error=>{
      if(finished)return;finished=true;clearTimeout(timer);chrome.tabs.onUpdated.removeListener(changed);
      if(error)reject(error);else resolve();
    };
    const inspect=tab=>{
      if(tab.url===url)done();
      else if(tab.url && tab.url!=='about:blank')done(Error('tab_changed_before_navigation'));
    };
    const changed=(tab,info,value)=>{if(tab===id)inspect(value);};
    const timer=setTimeout(()=>done(Error('tab_binding_failed')),5000);
    chrome.tabs.onUpdated.addListener(changed);
    chrome.tabs.get(id).then(inspect).catch(()=>done(Error('tab_binding_failed')));
  });
}
function readyFor(url,deadline) {
  const remaining=deadline-Date.now()-5000;
  if(remaining<=0)throw Error('deadline_before_submission');
  readyUrl=url;
  return next('content_ready',Math.min(60000,remaining));
}
async function content(action,data) {
  const reply=await chrome.tabs.sendMessage(tabId,{kind:"slingring",action,id:job,...data});
  if(!reply?.ok){submitted ||= !!reply?.submitted;const e=Error(reply?.code||"content_script_failed");e.url=canonical(reply?.url)?reply.url:null;throw e;}
  return reply;
}
chrome.runtime.onMessage.addListener((message,sender,reply)=>{
  if(message?.kind!=="slingring" || sender.id!==chrome.runtime.id || sender.tab?.id!==tabId || sender.frameId!==0 ||
      !sender.url?.startsWith("https://chatgpt.com/"))return false;
  if(message.event==="ready") {
    if(sender.url!==readyUrl)return false;
    const wait=waits.get("content_ready");if(wait){waits.delete("content_ready");wait.resolve({});}return false;
  }
  if(message.event==='progress' && message.id===job && !finished && stages.has(message.stage) &&
      (!targetUrl || sender.url===targetUrl || sender.url==='https://chatgpt.com/')) {
    progress(message.stage);return false;
  }
  if(message.event==="conversation" && message.id===job && submitted && canonical(message.url) &&
      (!targetUrl || message.url===targetUrl)) {
    native.postMessage({type:"conversation",job,url:message.url});return false;
  }
  if(message.event!=="will_submit" || message.id!==job || submitted || finished)return false;
  if(targetUrl && sender.url!==targetUrl){reply({ok:false});return false;}
  const ack=next("continue",10000);
  native.postMessage({type:"will_submit",job});
  ack.then(()=>{submitted=true;reply({ok:true});}).catch(()=>reply({ok:false}));
  return true;
});
async function run() {
  if(!token || !/^[a-f0-9-]{36}$/.test(token) || !job || job.length>128)throw Error("invalid_bootstrap");
  native=chrome.runtime.connectNative(HOST);
  native.onMessage.addListener(message=>{
    const wait=waits.get(message.type);if(wait){waits.delete(message.type);wait.resolve(message);}
  });
  native.onDisconnect.addListener(()=>{failWaits();});
  const ready=next("task",15000);
  native.postMessage({type:"hello",token,job});
  const task=await ready;
  if(task.job!==job || typeof task.prompt!=="string")throw Error("invalid_task");
  if(task.tab_url!==`about:blank#slingring-${token}`)throw Error('invalid_tab_bootstrap');
  if(!Number.isInteger(task.image_count) || task.image_count<0 || task.image_count>16)throw Error('invalid_image_count');
  if(task.chatgpt_url!=null){
    if(!canonical(task.chatgpt_url) || task.effort!=null)throw Error("invalid_followup");
    targetUrl=task.chatgpt_url;
  }
  const deadline=task.deadline_ms || Date.now()+120000;
  if(deadline-Date.now()<5000)throw Error('deadline_before_submission');
  // Bind the extension's tab to the Rust-owned CDP target before navigation.
  // A URL match after navigation cannot distinguish another same-URL tab.
  const tab=await chrome.tabs.create({url:task.tab_url,active:true});tabId=tab.id;
  await bootstrapCommitted(tabId,task.tab_url);
  const bound=next('tab_bound',10000);
  native.postMessage({type:'bind_tab',job});
  if((await bound).ok!==true)throw Error('tab_binding_failed');
  if((await chrome.tabs.get(tabId)).url!==task.tab_url)throw Error('tab_changed_before_navigation');
  const contentReady=readyFor('https://chatgpt.com/',deadline);
  progress('chat_loading');
  await chrome.tabs.update(tabId,{url:'https://chatgpt.com/'});
  await contentReady;
  if(targetUrl){
    await content('ensure_chat',{deadline_ms:deadline});
    const targetReady=readyFor(targetUrl,deadline);
    progress('conversation_loading');
    await chrome.tabs.update(tabId,{url:targetUrl});
    await targetReady;
  }
  const prepared=await content("prepare",{prompt:task.prompt,effort:task.effort,image_count:task.image_count,
    chatgpt_url:targetUrl,timeout_ms:task.timeout_ms,deadline_ms:task.deadline_ms});
  if(task.image_count){
    const uploaded=next("uploaded",45000);
    native.postMessage({type:"upload",job,selector:prepared.upload_selector});
    const reply=await uploaded;if(reply.ok!==true)throw Error("image_upload_failed");
  }
  const result=await content("submit",{});
  if(!canonical(result.url) || (targetUrl && result.url!==targetUrl) ||
      !["thinking","working","streaming"].includes(result.evidence))throw Error("invalid_result");
  const acknowledged=next("ack",10000);
  finished=true;
  native.postMessage({type:"started",job,url:result.url,evidence:result.evidence,effort:result.effort});
  await acknowledged;
  await chrome.tabs.remove(tabId);
  native.postMessage({type:"released",job});
  const own=await chrome.tabs.getCurrent();if(own?.id)await chrome.tabs.remove(own.id);
}
run().catch(async error=>{
  finished=true;
  try{native?.postMessage({type:"failed",job,code:safeCode(error),submitted,url:canonical(error.url)?error.url:null});}catch{}
  // Rust owns final cleanup, including failures and interrupted native channels.
  document.body.textContent="Slingring stopped. Inspect slingring queue for status.";
});
