param([string]$ScreenshotPath, [switch]$TestMobile)
$ErrorActionPreference = 'Stop'
$adb = 'C:/Users/Administrator/AppData/Local/Android/Sdk/platform-tools/adb.exe'
$appProcess = (& $adb shell pidof com.deskpet.app).Trim()
if (-not $appProcess) { throw 'DeskPet is not running on the device.' }
& $adb forward tcp:9223 ("localabstract:webview_devtools_remote_$appProcess") | Out-Null
$socket = [System.Net.WebSockets.ClientWebSocket]::new()
$timeout = [System.Threading.CancellationTokenSource]::new()
$timeout.CancelAfter(90000)
$script:rpcCounter = 0
function Invoke-DevTools([string]$Method, [hashtable]$Arguments) {
    $script:rpcCounter++
    $rpcId = $script:rpcCounter
    $json = @{ id = $rpcId; method = $Method; params = $Arguments } | ConvertTo-Json -Depth 8 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $socket.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, $timeout.Token).GetAwaiter().GetResult() | Out-Null
    do {
        $message = [System.IO.MemoryStream]::new()
        try {
            do {
                $buffer = [byte[]]::new(65536)
                $chunk = $socket.ReceiveAsync([ArraySegment[byte]]::new($buffer), $timeout.Token).GetAwaiter().GetResult()
                $message.Write($buffer, 0, $chunk.Count)
            } while (-not $chunk.EndOfMessage)
            $response = [System.Text.Encoding]::UTF8.GetString($message.ToArray()) | ConvertFrom-Json
        } finally { $message.Dispose() }
    } while ($response.id -ne $rpcId)
    if ($response.error) { throw ($response.error | ConvertTo-Json -Compress) }
    return $response.result
}
function Invoke-JavaScript([string]$Expression) {
    $result = Invoke-DevTools 'Runtime.evaluate' @{ expression = $Expression; returnByValue = $true; awaitPromise = $true }
    if ($result.exceptionDetails) { throw ($result.exceptionDetails | ConvertTo-Json -Depth 4) }
    return $result.result.value
}
try {
    $pages = (Invoke-WebRequest -UseBasicParsing -Uri 'http://127.0.0.1:9223/json' -TimeoutSec 10).Content | ConvertFrom-Json
    $page = @($pages)[0]
    $socket.ConnectAsync([Uri]$page.webSocketDebuggerUrl, $timeout.Token).GetAwaiter().GetResult()
    $expression = "JSON.stringify({title:document.title,url:location.href,ready:document.readyState,moods:typeof spriteCache==='undefined'?[]:Object.keys(spriteCache),active:typeof activeSprite==='undefined'?null:activeSprite,display:document.querySelector('#pet-sprite')?getComputedStyle(document.querySelector('#pet-sprite')).display:null})"
    $result = Invoke-DevTools 'Runtime.evaluate' @{ expression = $expression; returnByValue = $true }
    if ($result.exceptionDetails) { throw ($result.exceptionDetails | ConvertTo-Json -Depth 4) }
    $state = $result.result.value | ConvertFrom-Json
    $state | ConvertTo-Json -Depth 4
    if ($state.title -ne 'DeskPet' -or $state.moods.Count -ne 6 -or $state.display -ne 'block') {
        throw 'WebView did not load the six configured sprite sheets.'
    }
    if (([Uri]$state.url).Host -ne 'tauri.localhost') { throw 'WebView is still using a remote dev server.' }
    if ($TestMobile) {
        Write-Output 'Checking mobile controls...'
        $mobile = Invoke-JavaScript "Boolean(window.deskpetMobile && document.querySelector('#open-settings'))"
        if (-not $mobile) { throw 'Mobile UI missing.' }
        Invoke-JavaScript "document.querySelector('#bubble-close').click(); true" | Out-Null
        Invoke-JavaScript "window.__tapTrace=[];['pointerdown','pointerup','pointercancel'].forEach(type=>document.querySelector('#pet-wrap').addEventListener(type,e=>window.__tapTrace.push({type:e.type,button:e.button,primary:e.isPrimary,x:e.clientX,y:e.clientY})));true" | Out-Null
        $rect = (Invoke-JavaScript "JSON.stringify((()=>{const r=document.querySelector('#pet-wrap').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})())") | ConvertFrom-Json
        foreach ($tap in 1..2) {
            Write-Output "Dispatching touch $tap..."
            Invoke-DevTools 'Input.dispatchTouchEvent' @{ type='touchStart'; touchPoints=@(@{x=$rect.x;y=$rect.y}) } | Out-Null
            Invoke-DevTools 'Input.dispatchTouchEvent' @{ type='touchEnd'; touchPoints=@() } | Out-Null
            Start-Sleep -Milliseconds 90
        }
        Start-Sleep -Milliseconds 250
        $bubble = Invoke-JavaScript "JSON.stringify({visible:!document.querySelector('#mobile-bubble').hidden,text:document.querySelector('#mobile-bubble-text').textContent})"
        $bubbleState = $bubble | ConvertFrom-Json
        if (-not $bubbleState.visible -or -not $bubbleState.text) {
            Invoke-JavaScript "JSON.stringify({trace:window.__tapTrace,rect:document.querySelector('#pet-wrap').getBoundingClientRect().toJSON(),atPet:document.elementFromPoint($($rect.x),$($rect.y))?.outerHTML.slice(0,150)})"
            throw 'Double touch did not show bubble.'
        }
        Write-Output 'PASS: real double touch shows an in-page bubble.'
        # Wait for the real IPC call to finish; never print announcement content or keys.
        $finished = Invoke-JavaScript "new Promise(resolve=>{let n=0;const timer=setInterval(()=>{if(!document.querySelector('#mobile-trigger').disabled||++n>=200){clearInterval(timer);resolve(!document.querySelector('#mobile-trigger').disabled)}},250)})"
        if (-not $finished) { throw 'Agent did not finish within 50 seconds.' }
        Write-Output 'PASS: trigger_agent finished and the trigger button is available again.'
        Invoke-JavaScript "document.querySelector('#open-settings').click(); true" | Out-Null
        Start-Sleep -Milliseconds 500
        $opened = Invoke-JavaScript "document.querySelector('#settings-dialog').open && document.querySelector('#api-key').value === ''"
        if (-not $opened) { throw 'Settings dialog did not open with a masked/blank key field.' }
        # Save the existing values, with null API key to preserve the user's secret.
        Invoke-JavaScript "(async()=>{window.__deskpetSettingsBefore = await window.__TAURI__.core.invoke('get_settings'); document.querySelector('#settings-form').requestSubmit();return true})()" | Out-Null
        $saved = Invoke-JavaScript "new Promise(resolve=>{let n=0;const timer=setInterval(()=>{if(document.querySelector('#settings-status').textContent.includes('已保存')||++n>=120){clearInterval(timer);resolve(document.querySelector('#settings-status').textContent.includes('已保存'))}},250)})"
        if (-not $saved) { throw 'Settings did not save successfully.' }
        $same = Invoke-JavaScript "(async()=>JSON.stringify(await window.__TAURI__.core.invoke('get_settings')) === JSON.stringify(window.__deskpetSettingsBefore))()"
        if (-not $same) { throw 'Saving unchanged settings changed the configuration.' }
        $validated = Invoke-JavaScript "(async()=>{try{await window.__TAURI__.core.invoke('save_settings',{settings:{apiBase:'file:///invalid',model:'invalid',apiKey:null,embeddingModel:null}});return false}catch{return JSON.stringify(await window.__TAURI__.core.invoke('get_settings'))===JSON.stringify(window.__deskpetSettingsBefore)}})()"
        if (-not $validated) { throw 'Invalid settings were not rejected without mutation.' }
        Write-Output 'PASS: settings save through native IPC; blank key retained; invalid URL rejected.'
        Invoke-JavaScript "document.querySelector('#close-settings').click(); document.querySelector('#open-settings').click(); true" | Out-Null
        Start-Sleep -Milliseconds 300
        $reopened = Invoke-JavaScript "document.querySelector('#settings-dialog').open && document.querySelector('#api-key').value === '' && document.querySelector('#model-name').value === window.__deskpetSettingsBefore.model"
        if (-not $reopened) { throw 'Settings did not reload after closing.' }
        Write-Output 'PASS: saved settings reload on reopening; the key is never returned to the form.'
    }
    if ($ScreenshotPath) {
        $capture = Invoke-DevTools 'Page.captureScreenshot' @{ format = 'png' }
        [System.IO.File]::WriteAllBytes($ScreenshotPath, [Convert]::FromBase64String($capture.data))
        Write-Output "Screenshot saved: $ScreenshotPath"
    }
} finally {
    $socket.Dispose()
    $timeout.Dispose()
    & $adb forward --remove tcp:9223 | Out-Null
}
