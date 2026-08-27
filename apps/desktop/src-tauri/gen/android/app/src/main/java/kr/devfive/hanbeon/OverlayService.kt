package kr.devfive.hanbeon

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.graphics.PixelFormat
import android.os.Build
import android.os.IBinder
import android.provider.Settings
import android.view.Gravity
import android.view.WindowManager

/**
 * 4칸 컨트롤러를 다른 앱 위에 올린다.
 *
 * 이 제품은 '다른 앱을 조작하는 것'이라 우리 화면이 앞에 나오면 안 된다. 그래서
 * Activity가 아니라 포그라운드 Service가 `TYPE_APPLICATION_OVERLAY` 창을 직접 올린다.
 * Tauri의 안드로이드 지원은 Activity만 주기 때문에 여기는 순수 안드로이드로 짠다.
 *
 * 포그라운드 서비스로 두는 이유는 오래 살아야 하기 때문이다. 사용자는 스위치 말고
 * 다른 조작 수단이 없어서, 시스템이 우리를 조용히 죽이면 되살릴 방법이 없다.
 */
internal object OverlayStartupGate {
    fun canStart(canDrawOverlays: Boolean): Boolean = canDrawOverlays
}

class OverlayService : Service() {
    private var windows: WindowManager? = null
    private var controller: ControllerView? = null
    private var usb: UsbSwitch? = null
    private var ble: BleSwitch? = null
    private var highlight: HighlightView? = null
    private var lastStatusCode: String? = null
    private var statusListening = false

    /** 마지막으로 로그에 남긴 스캔 모드. 바뀔 때만 남기려고 들고 있다. */
    private var lastMode: String? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        startInForeground()
        if (!show()) {
            stopSelf()
            return
        }
        startTransports()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!OverlayStartupGate.canStart(Settings.canDrawOverlays(this))) {
            stopSelf()
            return START_NOT_STICKY
        }
        startTransports()
        return START_STICKY
    }

    /**
     * USB와 신뢰된 BLE를 함께 살리고, 코어에는 중재된 활성 원만 넘긴다.
     *
     * 서비스가 다시 시작돼도 스캐너와 전송 일꾼은 하나다. 눌림 판정은 코어의 몫이다.
     */
    private fun startTransports() {
        val usbSwitch =
            usb
                ?: UsbSwitch(this) { event ->
                    when (event) {
                        is UsbSwitch.Event.Connected ->
                            android.util.Log.i(TAG, "USB 스위치 붙음 ${event.name}")
                        UsbSwitch.Event.Disconnected ->
                            android.util.Log.w(TAG, "USB 스위치 빠짐")
                        UsbSwitch.Event.Press, UsbSwitch.Event.Release -> Unit
                    }
                }.also { usb = it }
        val bleSwitch =
            ble
                ?: BleSwitch(
                    context = this,
                    identityStore =
                        TrustedXiaoStore {
                            TrustedBleStore(java.io.File(filesDir, TRUSTED_BLE_FILE))
                                .load()
                                ?.identity
                                ?.takeIf { it.isNotBlank() }
                                ?.let(::TrustedXiaoIdentity)
                        },
                ).also { ble = it }
        OverlayTransportOwner.start(
            usb = usbSwitch,
            ble = bleSwitch,
            core = AndroidCoreInput,
            startScanner = { startCore() },
        )
        OverlayTransportOwner.setHints(TransportHints(blePermissionDenied = blePermissionDenied()))
        if (statusListening) return
        statusListening = true
        OverlayTransportOwner.status.addListener { snapshot ->
            MainActivity.emitTransportSnapshot(snapshot)
            if (snapshot.code == lastStatusCode) return@addListener
            lastStatusCode = snapshot.code
            android.util.Log.i(
                TAG,
                "입력 ${snapshot.code} active=${snapshot.active ?: "none"} usb=${snapshot.usb} ble=${snapshot.ble}",
            )
        }
    }

    private fun blePermissionDenied(): Boolean {
        val needed = BleSetupPolicy.runtimePermissions(android.os.Build.VERSION.SDK_INT)
        return needed.any {
            checkSelfPermission(it) != android.content.pm.PackageManager.PERMISSION_GRANTED
        }
    }

    /** 코어가 시키는 것을 실제로 한다. */
    private inner class Bridge : Core.Callbacks {
        override fun inject(action: Int): Boolean {
            val service = HanbeonAccessibilityService.instance
            if (service == null) {
                android.util.Log.w(TAG, "접근성 서비스가 꺼져 있습니다")
                return false
            }
            return when (action) {
                0 -> service.moveNext()
                1 -> service.movePrevious()
                2 -> service.select()
                // 설정 화면 열기는 키 주입이 아니라 우리 앱 내부 동작이다.
                3 -> true
                else -> false
            }
        }

        override fun undo(mapping: Int): Boolean =
            HanbeonAccessibilityService.instance?.back() ?: false

        override fun openSettings(unused: Int): Boolean {
            // Android는 하나의 Tauri WebView 위에 설정을 겹쳐 그린다. URL을
            // 바꾸면 Tauri IPC 브리지가 끊겨 빈 화면이 되므로 설정 이벤트만 보낸다.
            val intent =
                Intent(this@OverlayService, MainActivity::class.java)
                    .addFlags(
                        Intent.FLAG_ACTIVITY_NEW_TASK or
                            Intent.FLAG_ACTIVITY_SINGLE_TOP or
                            Intent.FLAG_ACTIVITY_CLEAR_TOP,
                    )
                    .putExtra(MainActivity.EXTRA_SCREEN, "settings")
            startActivity(intent)
            return true
        }

        override fun fitCells(extras: Int) {
            controller?.post { controller?.fitCells(extras) }
        }

        override fun cue(cue: Int) {
            // 소리는 아직 없다. 화면 강조만으로는 원칙 4(두 감각)를 못 지킨다.
        }

        override fun setSound(enabled: Boolean) = Unit

        override fun publishState(json: String) {
            logMode(json)
            val mode = runCatching { org.json.JSONObject(json).optString("mode") }.getOrNull()
            OverlayTransportOwner.setUserPausedFromCore(mode == "paused")
            controller?.post { controller?.render(json) }
        }

        override fun publishError(json: String) {
            android.util.Log.w(TAG, "오류 $json")
        }

        override fun publishInterval(json: String) {
            android.util.Log.i(TAG, "간격 $json")
            controller?.post { controller?.notice(json) }
        }

        override fun publishPreset(json: String) {
            android.util.Log.i(TAG, "앱별 칸 $json")
        }

        override fun saveProfile(json: String) {
            runCatching {
                java.io.File(filesDir, "profile.json").writeText(json)
            }
        }
    }

    /**
     * 스캔 모드가 바뀔 때만 남긴다.
     *
     * 커서 위치까지 남기면 주사 간격마다 한 줄씩 쌓여 로그가 못 쓰게 된다. 모드는
     * 사용자 입력에서만 바뀌므로 드물다. 정지인지 스캔 중인지를 화면만 보고
     * 구분하지 못해 실기에서 한참 헤맸다.
     */
    private fun logMode(json: String) {
        val mode = runCatching { org.json.JSONObject(json).optString("mode") }.getOrNull() ?: return
        if (mode == lastMode) return
        lastMode = mode
        android.util.Log.i(TAG, "모드 $mode")
    }

    /** 코어를 띄운다. 프로필이 없으면 코어가 기본값으로 시작한다. */
    private fun startCore() {
        val profile =
            runCatching { java.io.File(filesDir, "profile.json").readText() }
                .getOrDefault("{}")
        val logs = java.io.File(filesDir, "logs").apply { mkdirs() }
        val ok = Core.start(Bridge(), profile, logs.absolutePath)
        android.util.Log.i(TAG, "코어 ${if (ok) "시작됨" else "시작 실패"}")
    }

    private fun startInForeground() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL,
                "한번 실행 중",
                // 낮게 잡는다. 알림음이 울리면 스캔 소리와 섞여 사용자가 무엇이
                // 일어났는지 구분하지 못한다.
                NotificationManager.IMPORTANCE_LOW,
            ),
        )

        val notice =
            Notification.Builder(this, CHANNEL)
                .setContentTitle("한번이 실행 중입니다")
                .setContentText("스위치로 화면을 조작할 수 있습니다")
                .setSmallIcon(android.R.drawable.ic_menu_compass)
                .setOngoing(true)
                .build()

        startForeground(NOTIFICATION_ID, notice)
    }

    private fun show(): Boolean {
        if (!OverlayStartupGate.canStart(Settings.canDrawOverlays(this))) {
            // 권한 없이 창을 올리면 예외로 죽는다. 조용히 물러나고 안내 화면이
            // 권한을 받게 둔다.
            return false
        }

        val manager = getSystemService(WindowManager::class.java)
        // 창을 하나라도 올리기 전에 잡아 둔다. 컨트롤러를 올리다 실패해도
        // 먼저 올린 강조 창을 걷어낼 수 있어야 한다.
        windows = manager

        // 강조 테두리를 먼저 올린다. 나중에 올린 창이 위에 놓이므로, 순서가
        // 뒤집히면 대상 요소의 테두리가 컨트롤러를 가로질러 그려진다. 사용자가
        // 커서 위치를 읽는 화면이 가려지면 다음에 무엇이 올지 알 수 없다.
        showHighlight(manager)

        val view = ControllerView(this)

        // 폭을 못 박는다. WRAP_CONTENT로 두면 안쪽 칸의 가중치가 0폭으로 무너져
        // 컨트롤러가 화면 구석의 가는 띠가 되고, `Enter` 같은 글자는 줄바꿈된다.
        val width =
            android.util.TypedValue.applyDimension(
                android.util.TypedValue.COMPLEX_UNIT_DIP,
                CONTROLLER_WIDTH_DP,
                resources.displayMetrics,
            ).toInt()

        val params =
            WindowManager.LayoutParams(
                width,
                WindowManager.LayoutParams.WRAP_CONTENT,
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                // NOT_FOCUSABLE이 이 앱의 생명줄이다. 포커스를 받으면 우리가 옮기려는
                // 대상 앱의 포커스를 뺏어, 정작 조작할 것이 사라진다. 데스크톱에서
                // NSPanel과 WS_EX_NOACTIVATE로 애써 만드는 상태가 여기서는 기본이다.
                //
                // NOT_TOUCHABLE도 함께 건다. 컨트롤러는 **보여 주기만 한다** —
                // 조작은 스위치로 한다. 터치를 삼키면 두 가지가 깨진다. 선택할 때
                // 보내는 실제 두드림이 컨트롤러 뒤에 있는 요소 대신 우리에게
                // 먹히고, 보호자가 그 자리의 앱을 만질 수 없다.
                WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                    WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                    WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
                PixelFormat.TRANSLUCENT,
            )
        params.gravity = Gravity.BOTTOM or Gravity.END
        params.x = MARGIN
        params.y = MARGIN

        manager.addView(view, params)
        controller = view

        // 컨트롤러가 어디를 덮고 있는지 알린다. 선택할 때 보내는 두드림이 이
        // 자리에 떨어지면 대상 앱이 아니라 우리에게 먹힌다.
        view.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
            val at = IntArray(2)
            view.getLocationOnScreen(at)
            val rect =
                android.graphics.Rect(at[0], at[1], at[0] + view.width, at[1] + view.height)
            if (rect != HanbeonAccessibilityService.controllerBounds) {
                HanbeonAccessibilityService.controllerBounds = rect
                android.util.Log.i(TAG, "컨트롤러 자리 $rect")
            }
        }
        return true
    }

    /**
     * 고르고 있는 요소에 테두리를 그릴 창.
     *
     * 화면 전체를 덮되 **터치를 통과시킨다**(`FLAG_NOT_TOUCHABLE`). 안 그러면 이
     * 창이 대상 앱의 터치를 모두 삼켜서, 보호자가 화면을 만질 수 없게 된다.
     */
    private fun showHighlight(manager: WindowManager) {
        val view = HighlightView(this)
        val params =
            WindowManager.LayoutParams(
                WindowManager.LayoutParams.MATCH_PARENT,
                WindowManager.LayoutParams.MATCH_PARENT,
                WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                    WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                    WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS or
                    WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
                PixelFormat.TRANSLUCENT,
            )
        manager.addView(view, params)
        highlight = view

        HanbeonAccessibilityService.onFocusMoved = { bounds -> view.show(bounds) }
    }

    override fun onDestroy() {
        HanbeonAccessibilityService.onFocusMoved = null
        HanbeonAccessibilityService.controllerBounds = null
        highlight?.let { view -> runCatching { windows?.removeView(view) } }
        highlight = null
        OverlayTransportOwner.stop()
        Core.stop()
        usb = null
        ble = null
        statusListening = false
        controller?.let { view -> runCatching { windows?.removeView(view) } }
        controller = null
        windows = null
        super.onDestroy()
    }

    companion object {
        private const val TAG = "한번"
        private const val CHANNEL = "hanbeon.overlay"
        private const val NOTIFICATION_ID = 1
        private const val TRUSTED_BLE_FILE = "trusted-ble.json"

        /** 화면 가장자리와의 여백. 붙어 있어도 칸 자리는 읽힌다. */
        private const val MARGIN = 12

        /// 스위치 사용자는 곁눈으로도 자리를 알아봐야 하지만, 컨트롤러가 크면
        /// 그만큼 조작할 앱을 가린다. 접근성 서비스는 가려 두드릴 수 없는 요소를
        /// 스캔 대상에서 빼 버리므로 폭은 기호가 읽히는 최소한만 쓴다.
        private const val CONTROLLER_WIDTH_DP = 200f

        fun start(context: android.content.Context) {
            val intent = Intent(context, OverlayService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        fun stop(context: android.content.Context) {
            context.stopService(Intent(context, OverlayService::class.java))
        }
    }
}
