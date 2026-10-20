// A genuine isolated extension content listener, only on loopback fixture pages.
document.documentElement.setAttribute('data-bowser-api-content-ready','yes');
chrome.runtime.onMessage.addListener((message,_sender,reply)=>{
 if(message?.type!=='bowser-api-isolation'||typeof message.token!=='string'||message.token.length>128)return false;
 document.documentElement.setAttribute('data-bowser-api-message',message.token);
 reply({received:true,token:message.token});return false;
});
