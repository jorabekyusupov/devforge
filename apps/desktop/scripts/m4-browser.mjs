// Native-browser verification via the pinned Playwright CLI, never a TLS bypass.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const input=JSON.parse(await readFile(process.argv[2],'utf8'));
const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
const cli=path.join(repo,'node_modules/playwright/cli.js');
const session=`m4-${process.pid}`;const channel=process.platform==='win32'?'msedge':'chrome';
const config=path.join(input.root,'playwright-config.json');
await mkdir(path.join(repo,'output/playwright'),{recursive:true});
await writeFile(config,JSON.stringify({browser:{browserName:'chromium',isolated:true,launchOptions:{channel,headless:true},contextOptions:{ignoreHTTPSErrors:false}},outputDir:path.join(repo,'output/playwright'),timeouts:{navigation:30000,action:15000}}));
function command(...args){const output=execFileSync(process.execPath,[cli,`-s=${session}`,...args],{cwd:repo,encoding:'utf8',timeout:90000,maxBuffer:2*1024*1024,env:process.env});if(output.includes('### Error'))throw new Error(output);return output;}
const code=async body=>command('run-code',`async (page) => { ${body} }`);
try {
 command('open',input.url,'--browser',channel,'--config',config);
 command('snapshot');
 // Assertions and browser events require run-code; the page snapshot is captured first.
 await code(`await page.waitForFunction(()=>document.querySelector('#message')?.textContent==='BEFORE'); await page.evaluate(()=>{window.__m4DocumentToken=crypto.randomUUID();}); const response=await page.goto(${JSON.stringify(input.url)}); const tls=await response.securityDetails(); if(!tls?.issuer.includes('DevForge'))throw new Error('Native TLS issuer missing: '+JSON.stringify(tls)); if(!(await page.evaluate(()=>window.isSecureContext)))throw new Error('Secure context missing'); await page.waitForFunction(()=>document.querySelector('#message')?.textContent==='BEFORE'); await page.evaluate(()=>{window.__m4DocumentToken='persist-through-hmr';}); const cookies=await page.context().cookies(); if(!cookies.some(c=>c.name.endsWith('_session')&&c.secure&&c.httpOnly))throw new Error('Secure session cookie missing'); const sockets=[]; page.on('websocket',ws=>sockets.push(ws.url())); await page.reload(); await page.waitForFunction(()=>document.querySelector('#message')?.textContent==='BEFORE'); await page.evaluate(()=>{window.__m4DocumentToken='persist-through-hmr';}); await page.waitForTimeout(500); if(!sockets.some(url=>url.startsWith('wss://vite-')))throw new Error('WSS HMR connection missing: '+JSON.stringify(sockets));`);
 command('snapshot');
 await writeFile(input.message_file,"export const message='AFTER';\n");
 await code(`await page.waitForFunction(()=>document.querySelector('#message')?.textContent==='AFTER',{},{timeout:20000}); if(!(await page.evaluate(()=>window.__m4DocumentToken==='persist-through-hmr'&&window.__m4Loads===1)))throw new Error('HMR reloaded document');`);
 await code(`await page.locator('input[type=file]').setInputFiles(${JSON.stringify(input.upload_file)}); await Promise.all([page.waitForURL('**/upload'),page.getByRole('button',{name:'Upload',exact:true}).click()]); const result=JSON.parse(await page.locator('body').innerText()); if(!result.uploaded||!result.secure)throw new Error('Laravel multipart upload failed');`);
 command('snapshot');
 console.log('M4_BROWSER_REPORT:'+JSON.stringify({executed:true,browser:channel,native_tls_no_bypass:true,https_secure_context:true,secure_http_only_session_cookie:true,laravel_multipart_upload:true,vite_wss_hmr_without_reload:true}));
} finally {try{command('close');}catch{}}
