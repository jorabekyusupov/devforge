import {message} from './m4-message.js';
window.__m4Loads=(window.__m4Loads??0)+1;
document.getElementById('message').textContent=message;
if(import.meta.hot)import.meta.hot.accept('./m4-message.js',mod=>{document.getElementById('message').textContent=mod.message;});
