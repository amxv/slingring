/* This page exists only during a job. It never contacts a remote service. */
const HOST = "xyz.ashray.slingring";
const params = new URLSearchParams(location.hash.slice(1));
const token = params.get("token");
const job = params.get("job");
history.replaceState(null,"",location.pathname);
let native;
let tabId;
let submitted=false;
let finished=false;
const waits=new Map();
const next=(type,timeout=180000)=>new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>{waits.delete(type);reject(Error("native_timeout"));},timeout);
  waits.set(type,{resolve:value=>{clearTimeout(timer);resolve(value);},reject:error=>{clearTimeout(timer);reject(error);}});
});
function failWaits() { for(const wait of waits.values())wait.reject(Error("native_disconnected"));waits.clear(); }
const safeCode=e=>/^[a-z_]+$/.test(e?.message||"")?e.message:"extension_failed";
const canonical=url=>typeof url==="string" && /^https:\/\/chatgpt\.com\/c\/[a-zA-Z0-9-]+$/.test(url);
async function content(action,data) {
  const reply=await chrome.tabs.sendMessage(tabId,{kind:"slingring",action,id:job,...data});
  if(!reply?.ok){submitted ||= !!reply?.submitted;const e=Error(reply?.code||"content_script_failed");e.url=canonical(reply?.url)?reply.url:null;throw e;}
  return reply;
}
chrome.runtime.onMessage.addListener((message,sender,reply)=>{
  if(message?.kind!=="slingring" || sender.id!==chrome.runtime.id || sender.tab?.id!==tabId || sender.frameId!==0 ||
      !sender.url?.startsWith("https://chatgpt.com/"))return false;
  if(message.event==="ready") { const wait=waits.get("content_ready");if(wait){waits.delete("content_ready");wait.resolve({});}return false; }
  if(message.event!=="will_submit" || message.id!==job || submitted || finished)return false;
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
  const contentReady=next("content_ready",60000);
  const tab=await chrome.tabs.create({url:"about:blank",active:true});tabId=tab.id;
  await chrome.tabs.update(tabId,{url:"https://chatgpt.com/"});
  await contentReady;
  const prepared=await content("prepare",{prompt:task.prompt,effort:task.effort,image_count:task.image_count,timeout_ms:task.timeout_ms,deadline_ms:task.deadline_ms});
  if(task.image_count){
    const uploaded=next("uploaded",45000);
    native.postMessage({type:"upload",job,selector:prepared.upload_selector});
    const reply=await uploaded;if(reply.ok!==true)throw Error("image_upload_failed");
  }
  const result=await content("submit",{});
  if(!canonical(result.url) || !["thinking","working","streaming"].includes(result.evidence))throw Error("invalid_result");
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
