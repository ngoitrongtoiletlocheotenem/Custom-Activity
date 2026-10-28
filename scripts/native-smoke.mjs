import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import {createHash} from 'node:crypto';
const require=createRequire(import.meta.url);
const { chromium }=require(process.argv[2]);
const fixturePid=Number(process.argv[3]);
const workspace=process.argv[4];
const sdkEnabled=process.argv[5]==='sdk';
const observeSeconds=Math.max(0,Math.min(120,Number(process.argv[6])||0));
const iconTransitions=process.argv[7]==='true';
const uploadEndpoint=(process.argv[8]??'').replace(/\/+$/,'');
const publicImageBase=(process.argv[9]??'').replace(/\/+$/,'');
const uploadEnabled=Boolean(uploadEndpoint);
let uploadToken=process.env.CURRENTACTIVITY_SMOKE_UPLOAD_TOKEN;
delete process.env.CURRENTACTIVITY_SMOKE_UPLOAD_TOKEN;
if(uploadEnabled&&(!sdkEnabled||!publicImageBase||!uploadToken))throw new Error('R2 smoke requires SDK mode, public image base and an in-memory upload credential.');
const testDescription=sdkEnabled?'Testing CurrentActivity':'Native smoke activity';
const checks=[];
let browser;
const deadline=Date.now()+30000;
while(Date.now()<deadline){
  try { browser=await chromium.connectOverCDP('http://127.0.0.1:9224',{timeout:2000});break; }
  catch { await new Promise(resolve=>setTimeout(resolve,500)); }
}
if(!browser)throw new Error('WebView2 CDP did not become available within 30 seconds.');
const context=browser.contexts()[0];
let page;
try {
  for(let attempt=0;attempt<60;attempt++){
    page=context.pages().find(p=>!p.url().startsWith('devtools:'));
    if(page&&await page.evaluate(()=>Boolean(window.__TAURI_INTERNALS__?.invoke)).catch(()=>false))break;
    await new Promise(resolve=>setTimeout(resolve,500));
  }
  assert(page,'Native WebView page must exist');
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const assertIconDiagnostics=async(expectedApp)=>{
    let diagnostics=await invoke('get_icon_diagnostics');
    let expectedImage;
    if(uploadEnabled){
      const selected=(await invoke('get_activity_state')).selectedApplication;
      assert.equal(selected?.id,expectedApp.id,'Hash the currently selected application, not a stale enumeration snapshot');
      assert.ok(selected.iconDataUrl?.startsWith('data:image/png;base64,'));
      const png=Buffer.from(selected.iconDataUrl.split(',')[1],'base64');
      const digest=createHash('sha256').update(png).digest('hex');
      expectedImage=`${publicImageBase}/icons/${digest}.png`;
      const uploadDeadline=Date.now()+90000;
      while((diagnostics.resolvedIcon?.source!=='uploaded'||diagnostics.native?.largeImage!==expectedImage||!diagnostics.native?.callbackSuccessful)&&Date.now()<uploadDeadline){await page.waitForTimeout(250);diagnostics=await invoke('get_icon_diagnostics');}
      assert.equal(diagnostics.resolvedIcon?.source,'uploaded','Selected PNG must upload to the explicitly configured R2 host');
      assert.equal(diagnostics.resolvedIcon?.discordImage,expectedImage);
      const response=await fetch(expectedImage,{signal:AbortSignal.timeout(20000)});
      assert.equal(response.status,200,'Uploaded public PNG must be retrievable');
      assert.equal(createHash('sha256').update(Buffer.from(await response.arrayBuffer())).digest('hex'),digest,'Public PNG bytes must exactly match selected executable PNG');
    }
    assert.equal(diagnostics.selectedApplication?.executableName,expectedApp.executableName);
    assert.equal(diagnostics.localIcon.available,true,'Selected application keeps its locally extracted executable icon');
    const image=diagnostics.resolvedIcon?.discordImage;
    assert.equal(typeof image,'string');
    assert.ok(image.length>0,'Discord image must resolve to a configured asset or public URL');
    assert.ok(/^https:\/\//.test(image)||/^[a-zA-Z0-9_\-]+$/.test(image),'Never send a local path, file URI or data URI to Discord');
    assert.equal(diagnostics.native?.assetsCreated,true);
    assert.equal(diagnostics.native?.setAssetsCalled,true);
    assert.equal(diagnostics.native?.assetsAttached,true,'Native Activity getter must report attached assets');
    assert.equal(diagnostics.native?.largeImage,image,'Native Activity large-image getter matches resolver output');
    assert.equal(diagnostics.native?.largeText,expectedApp.name);
    assert.equal(diagnostics.native?.callbackReceived,true);
    assert.equal(diagnostics.native?.callbackSuccessful,true);
    return {executableName:expectedApp.executableName,source:diagnostics.resolvedIcon.source,discordImage:image,nativeLargeImage:diagnostics.native.largeImage,assetsAttached:true,callbackSuccessful:true};
  };
  const iconEvidence=[];
  await page.getByRole('button',{name:'Continue',exact:true}).click({timeout:30000});
  checks.push('Native first-run Continue keeps sharing disabled');
  assert.equal((await invoke('get_activity_state')).enabled,false);
  const settings=await invoke('get_settings');
  const expectedConfig=JSON.parse(await readFile(path.join(workspace,'config','app.json'),'utf8'));
  const expectedClientId=process.env.DISCORD_CLIENT_ID?.trim()?process.env.DISCORD_CLIENT_ID:expectedConfig.discordClientId;
  assert.equal(settings.discordClientId,expectedClientId,'Native settings must preserve the exact build-configured public Application ID string');
  assert.equal(settings.startWithWindows,false);
  assert.equal(settings.autoResume,false);
  const apps=await invoke('list_running_applications');
  const matches=apps.filter(a=>a.processIds.includes(fixturePid));
  assert.equal(matches.length,1,'Two visible fixture windows must deduplicate into one executable');
  const app=matches[0];
  assert.equal(app.name,'CurrentActivity Test App');
  assert.ok(app.iconDataUrl?.startsWith('data:image/png;base64,'),'Native executable icon must be extracted');
  checks.push('Visible-window enumeration, friendly ProductName, deduplication and executable icon extraction');
  await page.getByRole('button',{name:'Application',exact:true}).click();
  await page.getByRole('textbox',{name:'Search applications'}).fill('CurrentActivitySmokeFixture');
  await page.getByRole('button',{name:/CurrentActivity Test App CurrentActivitySmokeFixture.exe/}).click();
  const description=page.getByRole('textbox',{name:/Description/});
  await description.fill(testDescription);
  await page.getByRole('button',{name:/Update Presence/}).click();
  const descriptionDeadline=Date.now()+10000;
  while((await invoke('get_activity_state')).description!==testDescription&&Date.now()<descriptionDeadline)await page.waitForTimeout(100);
  assert.equal((await invoke('get_activity_state')).description,testDescription);
  if(uploadEnabled){
    let configured;
    try{configured=await invoke('configure_icon_upload',{enabled:true,consent:true,endpoint:uploadEndpoint,publicBase:publicImageBase,token:uploadToken});}
    catch{throw new Error('Explicit R2 smoke configuration failed.');}
    finally{uploadToken=undefined;}
    assert.equal(configured.iconUpload.enabled,true);
    assert.equal(configured.iconUpload.consentGranted,true);
    assert.equal(configured.iconUpload.credentialStored,true);
    checks.push('Explicit opt-in R2 host configured with per-user encrypted credential; token excluded from output');
  }
  await page.getByRole('switch',{name:'Share my current activity'}).click();
  let activity;
  for(let i=0;i<30;i++){activity=await invoke('get_activity_state');if(activity.enabled)break;await page.waitForTimeout(100);}
  assert.equal(activity.enabled,true);
  if(sdkEnabled){
    assert.match(settings.discordClientId,/^[1-9][0-9]*$/,'SDK smoke requires a configured public Application ID');
    const publicationDeadline=Date.now()+60000;
    do{activity=await invoke('get_activity_state');if(activity.published)break;await page.waitForTimeout(250);}while(Date.now()<publicationDeadline);
    assert.equal(activity.published,true,`Real SDK publication did not succeed: ${activity.discordState.status}: ${activity.discordState.message}`);
    assert.equal(activity.discordState.status,'connected');
    assert.equal(activity.description,'Testing CurrentActivity');
    checks.push('Real official SDK publication acknowledged; connected state and published=true for owned fixture');
    iconEvidence.push(await assertIconDiagnostics(app));
    assert.equal(iconEvidence.at(-1).source,uploadEnabled?'uploaded':'fallback');
    checks.push(uploadEnabled?'Unknown fixture PNG uploaded; public bytes hash and native SDK image getters/callback verified':'Unknown fixture keeps its local icon while native SDK getters confirm configured fallback image and successful callback');
    const publishedScreenshots=path.join(workspace,'docs','screenshots');
    await mkdir(publishedScreenshots,{recursive:true});
    await page.screenshot({path:path.join(publishedScreenshots,'native-sdk-published.png'),fullPage:true});
    console.log(`SDK PUBLICATION ACKNOWLEDGED. Holding the owned fixture presence for ${observeSeconds} seconds for read-only Discord observation.`);
    const observationDeadline=Date.now()+observeSeconds*1000;
    while(Date.now()<observationDeadline){
      await page.waitForTimeout(Math.min(1000,observationDeadline-Date.now()));
      const observed=await invoke('get_activity_state');
      assert.equal(observed.published,true,'SDK publication must remain active during observation');
      assert.equal(observed.discordState.status,'connected');
    }
    if(iconTransitions){
      const github=apps.find(a=>a.executableName.toLowerCase()==='githubdesktop.exe');
      const code=apps.find(a=>a.executableName.toLowerCase()==='code.exe');
      assert(github&&code,'IconTransitions requires GitHub Desktop and Visual Studio Code already running; this test never launches or stops them');
      for(const [selectedApp,text] of [[github,'Testing CurrentActivity (GitHub)'],[code,'Testing CurrentActivity (Code)'],[github,'Testing CurrentActivity (GitHub again)']]){
        await page.waitForTimeout(10000);
        await invoke('select_application',{appId:selectedApp.id});
        await invoke('set_presence',{appId:selectedApp.id,description:text});
        const transitionDeadline=Date.now()+30000;
        do{activity=await invoke('get_activity_state');if(activity.published&&activity.selectedApplication?.id===selectedApp.id&&activity.description===text)break;await page.waitForTimeout(250);}while(Date.now()<transitionDeadline);
        assert.equal(activity.published,true);
        assert.equal(activity.selectedApplication?.id,selectedApp.id);
        assert.equal(activity.description,text);
        iconEvidence.push(await assertIconDiagnostics(selectedApp));
      }
      assert.equal(iconEvidence[1].discordImage,iconEvidence[3].discordImage,'Returning to GitHub restores its icon after Code and description changes');
      const expectedSource=uploadEnabled?'uploaded':'external_url';
      assert.equal(iconEvidence[1].source,expectedSource);
      assert.equal(iconEvidence[2].source,expectedSource);
      assert.equal(iconEvidence[3].source,expectedSource);
      assert.notEqual(iconEvidence[1].discordImage,iconEvidence[2].discordImage,'GitHub and Code must have distinct mapped icons');
      checks.push('GitHub -> Code -> GitHub with explicit description changes: resolver/native getter agreement and successful SDK callbacks');
      await page.waitForTimeout(10000);
      assert.doesNotThrow(()=>process.kill(fixturePid,0),'Owned fixture exited unexpectedly before restoration');
      console.log('Owned fixture process still exists before restoration.');
      await invoke('list_running_applications');
      await invoke('select_application',{appId:app.id});
      await invoke('set_presence',{appId:app.id,description:testDescription});
      assert.equal((await invoke('get_activity_state')).published,true);
      iconEvidence.push(await assertIconDiagnostics(app));
    }
    // Preserve the real Application ID. Only isolated non-startup settings change.
    await page.getByRole('button',{name:'Open settings'}).click();
    await page.getByRole('switch',{name:/Start minimized/}).click();
    await page.getByRole('button',{name:'Save preferences'}).click();
    const settingsDeadline=Date.now()+10000;
    while(!(await invoke('get_settings')).startMinimized&&Date.now()<settingsDeadline)await page.waitForTimeout(100);
    assert.equal((await invoke('get_settings')).startMinimized,true);
    assert.equal((await invoke('get_settings')).discordClientId,settings.discordClientId);
    await page.getByRole('button',{name:'Back to activity'}).click();
    checks.push('Native settings persist without replacing the configured public Application ID');
    const disabled=await invoke('set_sharing_enabled',{enabled:false});
    assert.equal(disabled.enabled,false);
    assert.equal(disabled.published,false);
    checks.push('Real SDK clear completes successfully when sharing is disabled');
    await invoke('set_sharing_enabled',{enabled:true});
    const republishDeadline=Date.now()+30000;
    do{activity=await invoke('get_activity_state');if(activity.published)break;await page.waitForTimeout(250);}while(Date.now()<republishDeadline);
    assert.equal(activity.published,true,'Fixture presence must republish before testing process closure');
    checks.push('Real SDK presence republishes after explicit re-enable');
  } else {
  assert.equal(activity.published,false,'SDKless test must not claim Discord publication');
  for(let i=0;i<40;i++){activity=await invoke('get_activity_state');if(activity.discordState.status==='error')break;await page.waitForTimeout(100);}
  if (settings.discordClientId === '') {
    assert.match(activity.discordState.message,/Application ID/i);
    await page.getByRole('alert').filter({hasText:/Application ID/i}).waitFor();
    checks.push('Missing Application ID produces a visible actionable native error');
  } else {
    assert.match(settings.discordClientId,/^[1-9][0-9]*$/,'Build-configured Application ID must be a positive numeric identifier');
    assert.match(activity.discordState.message,/SDK/i);
    await page.getByRole('alert').filter({hasText:/SDK/i}).waitFor();
    checks.push('Build-configured public Application ID loads; missing official SDK produces a visible actionable native error');
  }
  await page.getByRole('button',{name:'Open settings'}).click();
  await page.getByRole('textbox',{name:'Discord application ID'}).fill('123456789012345678');
  await page.getByRole('button',{name:'Save preferences'}).click();
  for(let i=0;i<40;i++){if((await invoke('get_settings')).discordClientId==='123456789012345678')break;await page.waitForTimeout(100);}
  assert.equal((await invoke('get_settings')).discordClientId,'123456789012345678');
  await page.getByRole('button',{name:'Back to activity'}).click();
  for(let i=0;i<80;i++){activity=await invoke('get_activity_state');if(/SDK/i.test(activity.discordState.message))break;await page.waitForTimeout(100);}
  assert.match(activity.discordState.message,/SDK/i);
  assert.equal(activity.published,false);
  await page.getByRole('alert').filter({hasText:/SDK/i}).waitFor();
  checks.push('Settings save roundtrip and visible missing official SDK error; no publication');
  checks.push('Native picker selection, explicit description apply and sharing toggle without false publication');
  await page.getByRole('switch',{name:'Share my current activity'}).click();
  await invoke('set_sharing_enabled',{enabled:false});
  assert.equal((await invoke('get_activity_state')).published,false);
  await invoke('set_sharing_enabled',{enabled:true});
  }
  process.kill(fixturePid);
  const stopDeadline=Date.now()+(sdkEnabled?20000:8000);
  do{activity=await invoke('get_activity_state');if(!activity.selectedApplicationRunning)break;await page.waitForTimeout(250);}while(Date.now()<stopDeadline);
  assert.equal(activity.selectedApplicationRunning,false);
  assert.equal(activity.published,false);
  assert.equal(activity.resumeRequired,true);
  if(sdkEnabled)assert.notEqual(activity.discordState.status,'error','Selected-app closure must not hide an SDK clear error');
  checks.push('Owned selected application closes: presence cleared and manual resume required');
  await invoke('clear_presence');
  assert.equal((await invoke('get_activity_state')).enabled,false);
  checks.push('Clear Presence disables sharing');
  const target=path.join(workspace,'docs','screenshots');await mkdir(target,{recursive:true});
  await page.screenshot({path:path.join(target,sdkEnabled?'native-sdk-main.png':'native-main.png'),fullPage:true});
  const result={passed:true,mode:sdkEnabled?'sdk':'offline',r2UploadVerified:uploadEnabled,checks,iconEvidence,discordPublished:sdkEnabled,note:sdkEnabled?'Actual Windows debug executable + WebView2 + official Discord SDK callbacks and native Activity getters. Owned fixture activity was published and cleared; Discord client rendering was not visually verified.':'Actual Windows debug executable + WebView2; owned WinForms fixture. Official Discord SDK and live Discord publication are not exercised.'};
  await writeFile(path.join(workspace,'docs',sdkEnabled?'native-sdk-smoke-results.json':'native-smoke-results.json'),JSON.stringify(result,null,2));
  console.log(JSON.stringify(result));
} finally {
  uploadToken=undefined;
  if(page)await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('clear_presence')).catch(()=>{});
  // Disconnect CDP; PowerShell owns and terminates only the smoke-created processes.
  await browser.close();
}


