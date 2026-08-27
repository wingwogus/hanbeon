package kr.devfive.hanbeon

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * 검증용 통로.
 *
 * 스캔 상태기계(Rust 코어)가 아직 안 붙어서 각 동작을 손으로 불러 봐야 한다.
 * `>`가 정말 `Tab`을 대신할 수 있는지가 이 제품의 핵심 가설이라, 코어를 붙이기
 * 전에 이것부터 확인한다.
 *
 *     adb shell am broadcast -n kr.devfive.hanbeon/.DebugActionReceiver -e do next
 *
 * 코어가 붙으면 이 파일과 매니페스트의 receiver를 함께 지운다.
 */
class DebugActionReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) {
        val service = HanbeonAccessibilityService.instance
        if (service == null) {
            Log.w(TAG, "접근성 서비스가 꺼져 있습니다. 설정에서 켜 주세요.")
            return
        }

        val what = intent.getStringExtra("do")
        val ok =
            when (what) {
                "next" -> service.moveNext()
                "prev" -> service.movePrevious()
                "select" -> service.select()
                "back" -> service.back()
                else -> {
                    Log.w(TAG, "모르는 동작: $what")
                    return
                }
            }

        Log.i(TAG, "$what -> ${if (ok) "됨" else "안 됨"} (앞: ${HanbeonAccessibilityService.foreground})")
    }

    private companion object {
        const val TAG = "한번"
    }
}
