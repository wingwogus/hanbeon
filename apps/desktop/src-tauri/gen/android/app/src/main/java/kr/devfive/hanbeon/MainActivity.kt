package kr.devfive.hanbeon

import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import org.json.JSONObject

/**
 * Tauri 웹뷰 Activity. 앱의 기본 화면은 설정이다.
 *
 * 오버레이 서비스가 extra `screen`으로 어느 화면을 보여줄지 정한다:
 * - "floating" → floating 컨트롤러(index.html)
 * - 그 외/없음  → 설정 화면(settings/index.html)
 *
 * wry가 만든 RustWebView는 onWebViewCreate()로 전달되므로 여기서 참조를 저장해
 * 나중에 loadUrlMainThread()로 화면을 전환할 수 있게 한다.
 *
 * 블루투스 권한 요청은 이 보이는 Activity에서만 한다. 서비스 백그라운드에서
 * 띄우지 않고, 거절 뒤에 자동으로 다시 묻지 않는다.
 */
class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private var currentScreen: String? = null
  private var scanCallback: ScanCallback? = null
  private val scanGenerations = BleScanGenerations()
  private val pendingByToken = linkedMapOf<String, PendingCandidate>()
  private val setup by lazy {
    BleSetupController(
      store = TrustedBleStore(java.io.File(filesDir, TRUSTED_FILE)),
      apiLevel = Build.VERSION.SDK_INT,
      hasBleHardware = packageManager.hasSystemFeature(PackageManager.FEATURE_BLUETOOTH_LE),
      adapterOn = { adapter()?.isEnabled == true },
      permissionGranted = { hasRuntimePermissions() },
      scan = { onResult -> beginPlatformScan(onResult) },
      usbUsable = { OverlayTransportOwner.usbUsable() },
    )
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    navigateByIntent(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    navigateByIntent(intent)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    this.webView = webView
    activeWebView = webView as? RustWebView
    webView.addJavascriptInterface(BleSetupBridge(), JS_INTERFACE)
    navigateByIntent(intent)
    publishSnapshot()
  }

  override fun onResume() {
    super.onResume()
    publishSnapshot()
  }

  override fun onDestroy() {
    stopPlatformScan()
    if (activeWebView === webView) activeWebView = null
    super.onDestroy()
  }

  override fun onRequestPermissionsResult(
    requestCode: Int,
    permissions: Array<out String>,
    grantResults: IntArray,
  ) {
    super.onRequestPermissionsResult(requestCode, permissions, grantResults)
    if (requestCode != BleSetupPolicy.PERMISSION_REQUEST) return
    val granted = grantResults.isNotEmpty() && grantResults.all { it == PackageManager.PERMISSION_GRANTED }
    setup.notePermissionResult(granted)
    publishSnapshot()
  }

  private fun navigateByIntent(intent: Intent?) {
    val screen = intent?.getStringExtra(EXTRA_SCREEN) ?: "settings"
    intent?.removeExtra(EXTRA_SCREEN)
    if (screen == currentScreen) return

    // wry는 상대경로를 그대로 loadUrl하면 DNS 조회로 실패한다. 반드시
    // tauri asset origin 절대 URL을 써야 한다.
    val target =
      when (screen) {
        "floating" -> "http://tauri.localhost/index.html"
        else -> "http://tauri.localhost/settings/"
      }
    (webView as? RustWebView)?.loadUrlMainThread(target) ?: return
    currentScreen = screen
  }

  private fun publishSnapshot() {
    emitToWebView(BleSetupPolicyEvent, setup.snapshot().toJson(), "__hanbeonBleSetup")
  }

  private fun adapter(): BluetoothAdapter? =
    getSystemService(BluetoothManager::class.java)?.adapter

  private fun hasRuntimePermissions(): Boolean {
    val needed = BleSetupPolicy.runtimePermissions(Build.VERSION.SDK_INT)
    return needed.all { permission ->
      ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED
    }
  }

  private fun requestPermissionsFromSetup() {
    if (!setup.requestPermissionIfAllowed()) {
      publishSnapshot()
      return
    }
    ActivityCompat.requestPermissions(
      this,
      setup.runtimePermissions(),
      BleSetupPolicy.PERMISSION_REQUEST,
    )
  }

  private fun beginPlatformScan(onResult: (List<BleCandidateView>) -> Unit) {
    if (!hasRuntimePermissions()) {
      onResult(emptyList())
      return
    }
    val scanner = adapter()?.bluetoothLeScanner
    if (scanner == null) {
      onResult(emptyList())
      return
    }
    stopPlatformScan()
    val generation = scanGenerations.begin()
    pendingByToken.clear()
    val callback =
      object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
          accept(result)
        }

        override fun onBatchScanResults(results: MutableList<ScanResult>) {
          results.forEach(::accept)
        }

        override fun onScanFailed(errorCode: Int) {
          if (scanGenerations.claimCallback(generation)) {
            stopPlatformScan()
            onResult(pendingByToken.values.map { it.view })
          }
        }

        private fun accept(result: ScanResult) {
          val record = result.scanRecord
          val name = record?.deviceName ?: result.device.name
          val uuids =
            (record?.serviceUuids ?: emptyList()).map { it.uuid.toString() }
          if (!BleSetupPolicy.isTrustedCandidate(name, uuids)) return
          val address = result.device.address ?: return
          val token = "ble-${pendingByToken.size + 1}"
          if (pendingByToken.values.any { it.identity == address }) return
          val view = BleSetupPolicy.candidateView(token, name, address)
          pendingByToken[token] = PendingCandidate(address, view)
        }
      }
    scanCallback = callback
    val settings =
      ScanSettings.Builder()
        .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
        .build()
    val filter =
      ScanFilter.Builder()
        .setDeviceName(BleSetupPolicy.TRUSTED_NAME)
        .build()
    runCatching { scanner.startScan(listOf(filter), settings, callback) }
      .onFailure {
        onResult(emptyList())
        return
      }
    webView?.postDelayed(
      {
        if (!scanGenerations.claimTimeout(generation)) return@postDelayed
        stopPlatformScan()
        onResult(pendingByToken.values.map { it.view })
        publishSnapshot()
      },
      SCAN_MS,
    )
  }

  private fun stopPlatformScan() {
    scanGenerations.stop()
    val callback = scanCallback ?: return
    scanCallback = null
    runCatching { adapter()?.bluetoothLeScanner?.stopScan(callback) }
  }

  @Suppress("unused")
  fun transportStatusSnapshot(): String = OverlayTransportOwner.snapshotJson()

  @Suppress("unused")
  fun bleSetupSnapshot(): String = setup.snapshot().toJson()

  @Suppress("unused")
  fun bleSetupRequestPermission(): String {
    webView?.post { requestPermissionsFromSetup() }
    return setup.snapshot().toJson()
  }

  @Suppress("unused")
  fun bleSetupScan(): String {
    val next = setup.beginScan()
    webView?.post { publishSnapshot() }
    return next.toJson()
  }

  @Suppress("unused")
  fun bleSetupSelect(token: String): String {
    val pending = pendingByToken[token] ?: return setup.snapshot().toJson()
    val next = setup.select(token, pending.identity, pending.view.label)
    pendingByToken.clear()
    webView?.post { publishSnapshot() }
    return next.toJson()
  }

  @Suppress("unused")
  fun bleSetupRevoke(): String {
    val next = setup.revoke()
    pendingByToken.clear()
    webView?.post { publishSnapshot() }
    return next.toJson()
  }

  private inner class BleSetupBridge {
    @JavascriptInterface
    fun snapshot(): String = bleSetupSnapshot()

    @JavascriptInterface
    fun requestPermission(): String = bleSetupRequestPermission()

    @JavascriptInterface
    fun scan(): String = bleSetupScan()

    @JavascriptInterface
    fun select(token: String): String = bleSetupSelect(token)

    @JavascriptInterface
    fun revoke(): String = bleSetupRevoke()
  }

  private data class PendingCandidate(
    val identity: String,
    val view: BleCandidateView,
  )

  companion object {
    @Volatile private var activeWebView: RustWebView? = null

    fun emitTransportSnapshot(snapshot: TransportStatusSnapshot) {
      emitToWebView(
        TransportFrontendEvent.from(snapshot).name,
        snapshot.toJson(),
        "__hanbeonTransportStatus",
      )
    }

    private fun emitToWebView(event: String, json: String, slot: String) {
      val encoded = JSONObject.quote(json)
      activeWebView?.post {
        activeWebView?.evaluateJavascript(
          """
          (function() {
            var snapshot = JSON.parse($encoded);
            window.$slot = snapshot;
            window.dispatchEvent(new CustomEvent('$event', { detail: snapshot }));
          })()
          """.trimIndent(),
          null,
        )
      }
    }

    const val EXTRA_SCREEN = "screen"
    private const val JS_INTERFACE = "hanbeonBleSetup"
    private const val TRUSTED_FILE = "trusted-ble.json"
    private const val SCAN_MS = 8_000L
    private const val BleSetupPolicyEvent = "ble://setup"
  }
}
